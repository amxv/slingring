#!/usr/bin/env python3
"""Exercise editor text in real Chromium using an empty, disposable profile.

No ChatGPT page is opened and no messages are sent. This complements jsdom,
which does not implement Chromium's rendered innerText paragraph behavior.
"""

import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import uuid


FIXTURE = r"""
if (location.href !== 'about:blank') throw Error('fixture must stay on about:blank');
document.body.innerHTML = '<main><form><div id="prompt-textarea" contenteditable="true" aria-label="Ask ChatGPT" style="white-space:pre-wrap"></div></form></main>';
"""

CHECKS = r"""
(() => {
  const d = globalThis.__slingringDOM;
  const input = document.getElementById('prompt-textarea');
  const read = d.editorText || d.text;
  const passed = [];
  function check(name, actual, expected) {
    if (actual !== expected) throw Error(name + ': synthetic editor text differed');
    passed.push(name);
  }
  const fixtures = [
    ['empty paragraph placeholder', '<p><br></p>', ''],
    ['paragraph boundary', '<p>First line</p><p>Second line</p>', 'First line\nSecond line'],
    ['empty paragraph between lines', '<p>First</p><p><br></p><p>Third</p>', 'First\n\nThird'],
    ['leading empty paragraph', '<p><br></p><p>Second</p>', '\nSecond'],
    ['multiple leading empty paragraphs', '<p><br></p><p><br></p><p>Third</p>', '\n\nThird'],
    ['trailing empty paragraph', '<p>First</p><p><br></p>', 'First\n'],
    ['explicit line break', 'First<br>Second', 'First\nSecond'],
    ['nested block boundary', '<div>First<div>Second</div></div>', 'First\nSecond'],
    ['inline formatting', '<p><strong>First</strong> line</p><p><em>Second</em></p>', 'First line\nSecond'],
    ['meaningful spaces', '<p>  First  line  </p>', '  First  line  '],
  ];
  for (const [name, html, expected] of fixtures) {
    input.innerHTML = html;
    check(name, read(input), expected);
  }
  input.innerHTML = '<p><br></p>';
  const prompt = 'First line\nSecond line';
  d.fill(prompt);
  check('native insertText round trip', read(input), prompt);
  input.innerHTML = '<p>First line</p><p>Second line</p>';
  check('equivalent editor rerender', read(input), prompt);
  input.textContent = 'Unrelated synthetic draft';
  let rejected = false;
  try { d.fill('New synthetic prompt'); } catch (error) { rejected = error.message === 'unrelated_draft_present'; }
  check('unrelated draft rejected', rejected, true);
  check('unrelated draft preserved', read(input), 'Unrelated synthetic draft');
  input.textContent = prompt;
  rejected = false;
  try { d.fill(prompt); } catch (error) { rejected = error.message === 'unrelated_draft_present'; }
  check('identical preexisting draft rejected', rejected, true);
  if (d.promptState) {
    input.textContent = 'Unrelated synthetic draft';
    check('changed prompt classified', d.promptState(prompt), 'prompt_changed');
    input.innerHTML = '<p><br></p>';
    check('lost prompt classified', d.promptState(prompt), 'prompt_lost');
    input.remove();
    check('missing composer classified', d.promptState(prompt), 'composer_unavailable');
  }
  return {passed, count: passed.length, origin: location.href};
})()
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--browser', required=True, help='Pinned agent-browser-browse executable')
    parser.add_argument('--dom', type=Path, default=Path(__file__).resolve().parents[1] / 'extension/dom.js')
    args = parser.parse_args()
    browser = Path(args.browser).expanduser().resolve()
    if not browser.is_file() or browser.name == 'agent-browser':
        parser.error('Pass the pinned browse helper, not upstream agent-browser')
    dom = args.dom.read_text()
    session = 'slingring-dom-test-' + uuid.uuid4().hex[:12]
    root = Path(tempfile.mkdtemp(prefix='slingring-dom-test-'))
    profile = root / 'empty-profile'
    profile.mkdir()
    command = [str(browser), '--session', session, '--profile', str(profile),
               '--idle-timeout', '2m', '--json']

    def run(arguments, script=None):
        process = subprocess.run(command + arguments, input=script, capture_output=True,
                                 text=True, timeout=70)
        try:
            response = json.loads(process.stdout)
        except json.JSONDecodeError as error:
            raise RuntimeError('Browse helper returned invalid JSON') from error
        if process.returncode or response.get('success') is not True:
            raise RuntimeError('Synthetic browser operation failed: ' + str(response.get('error', process.returncode)))
        return response.get('data', {})

    closed = False
    try:
        run(['open', 'about:blank'])
        result = run(['eval', '--stdin'], FIXTURE + '\n' + dom + '\n' + CHECKS)
        evidence = result.get('result')
        if not isinstance(evidence, dict) or not evidence.get('passed'):
            raise RuntimeError('Synthetic editor checks did not produce evidence')
        marker = 'about:blank#slingring-' + uuid.uuid4().hex
        created = run(['tab', 'new', marker])
        target = created.get('targetId')
        if not isinstance(target, str) or not target:
            raise RuntimeError('Browser did not identify the synthetic task tab')
        tabs = run(['tab', 'list']).get('tabs', [])
        matching = [tab for tab in tabs if tab.get('url') == marker]
        if len(matching) != 1 or matching[0].get('targetId') != target:
            raise RuntimeError('Nonce URL did not identify exactly one browser target')
        destination = 'about:blank#fixture-destination'
        moved = run(['open', destination])
        if moved.get('targetId') != target:
            raise RuntimeError('Navigation changed the pinned synthetic target')
        tabs = run(['tab', 'list']).get('tabs', [])
        owned = [tab for tab in tabs if tab.get('targetId') == target]
        if len(owned) != 1 or owned[0].get('url') != destination:
            raise RuntimeError('Pinned target metadata lost the destination')
        evidence['passed'].extend(['unique nonce target', 'target identity survives navigation'])
        evidence['count'] = len(evidence['passed'])
        print(json.dumps(evidence, indent=2))
    finally:
        try:
            run(['close'])
            closed = True
        finally:
            if closed:
                shutil.rmtree(root)
            else:
                print(f'Cleanup incomplete. Close only session {session}; fixture profile retained at {root}.')


if __name__ == '__main__':
    main()
