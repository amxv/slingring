---
title: Local input and images
description: Prompts, local attachments, and safe retry behavior.
order: 2
category: Start
summary: Prompts, local attachments, and safe retry behavior.
---

## Prompt sources

```bash
slingring new-task "Compare these approaches"
slingring new-task --prompt-file ./brief.txt
cat brief.txt | slingring new-task --prompt-file -
slingring new-task "Explain this diagram" --image ./diagram.png --image ./detail.jpg
```

Image paths are local CLI inputs only. They must be readable on the Mac running Slingring. Remote clients cannot request file uploads or supply filesystem paths. Prompts and attached images are sent to ChatGPT.

## Idempotency and uncertainty

```bash
slingring new-task --prompt-file ./brief.txt --request-id migration-review-01 --json
```

Reuse an ID only for the identical request. A different payload with the same ID is rejected. Both local and remote work enter one bounded serial queue.

`failed` reports a failure; `uncertain` means submission may have occurred. Inspect the job and ChatGPT before creating another request. Do not blindly resubmit after a timeout, disconnect, or daemon crash. There is no automatic resubmission of submitted or uncertain work.
