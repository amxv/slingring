use rustix::io::Errno;
use rustix::process::{
    Pid, Signal, WaitId, WaitIdOptions, WaitIdStatus, getpgid, getpid, getsid, kill_process_group,
    waitid,
};
use std::{
    io,
    os::unix::process::CommandExt,
    process::{Child, Command, ExitStatus},
    thread,
    time::{Duration, Instant},
};

pub struct GroupLease {
    child: Option<Child>,
    leader: Pid,
}

impl GroupLease {
    /// The daemon must have checked setsid before serving commands.
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        if getsid(None)? != getpid() {
            return Err(io::Error::other("private daemon session required"));
        }
        // std propagates process_group setup errors through spawn. It is not
        // a user-provided numeric group: the child becomes its own leader.
        let child = command.process_group(0).spawn()?;
        let leader = Pid::from_child(&child);
        let owned = Self {
            child: Some(child),
            leader,
        };
        // waitid before returning confirms direct wait ownership even if exec
        // succeeded and the child immediately became a zombie.
        owned.observe()?;
        Ok(owned)
    }

    pub fn id(&self) -> u32 {
        self.leader.as_raw_nonzero().get() as u32
    }

    pub fn observe(&self) -> io::Result<Option<WaitIdStatus>> {
        if self.child.is_none() {
            return Err(io::Error::other("group lease ended"));
        }
        let flags = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
        loop {
            match waitid(WaitId::Pid(self.leader), flags) {
                Err(Errno::INTR) => continue,
                result => return Ok(result?),
            }
        }
    }

    fn signal(&self, signal: Signal) -> Result<(), Errno> {
        // The sole Child and no competing wait preserve the numeric ID even
        // after natural exit. A wait ownership error must never lead to killpg.
        if self.child.is_none() {
            return Err(Errno::CHILD);
        }
        let flags = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
        loop {
            match waitid(WaitId::Pid(self.leader), flags) {
                Err(Errno::INTR) => continue,
                result => {
                    result?;
                    break;
                }
            }
        }
        if getpgid(Some(self.leader))? != self.leader || getsid(Some(self.leader))? != getpid() {
            return Err(Errno::PERM);
        }
        kill_process_group(self.leader, signal)
    }

    /// Consume the direct child only after an observed ordinary exit.
    pub(super) fn reap_without_signal(&mut self, budget: Duration) -> io::Result<ExitStatus> {
        let deadline = Instant::now() + budget;
        while self.observe()?.is_none() {
            if Instant::now() >= deadline {
                return Err(io::Error::other("graceful root exit timed out"));
            }
            thread::sleep(Duration::from_millis(10));
        }
        self.child
            .take()
            .ok_or_else(|| io::Error::other("root already consumed"))?
            .wait()
    }

    pub fn cleanup(&mut self, grace: Duration) -> io::Result<()> {
        if self.child.is_none() {
            return Err(io::Error::other("group lease ended"));
        }
        let _ = self.signal(Signal::TERM);
        let end = Instant::now() + grace;
        while Instant::now() < end {
            thread::sleep(Duration::from_millis(10));
        }
        let _ = self.signal(Signal::KILL);
        // Do not block indefinitely if proof/signal failed. The process may
        // already be a zombie or merely unknown; never fall back to raw PID.
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.observe()?.is_none() {
            if Instant::now() >= deadline {
                return Err(io::Error::other("owned root exit unverified"));
            }
            thread::sleep(Duration::from_millis(10));
        }
        // Consume authority before reaping, so neither failed wait nor Drop
        // can signal a reused numeric group.
        let mut child = self.child.take().unwrap();
        child.wait()?;
        Ok(())
    }
}
impl Drop for GroupLease {
    fn drop(&mut self) {
        if self.child.is_some() {
            let _ = self.cleanup(Duration::ZERO);
        }
    }
}
