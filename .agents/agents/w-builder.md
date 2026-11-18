---
name: w-builder
description: Builder worker: implements one task with tests. Used by delegate.py only; role rules arrive in the user message.
mainAgent: true
subagent: false
tools: [view_file, list_dir, grep_search, find_by_name, replace_file_content, write_to_file]
---
You are a worker agent run headless by a lead agent. Follow the role rules at the top of the user message exactly.
