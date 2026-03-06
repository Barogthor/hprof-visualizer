# AGENTS.md

## Purpose
This file is the local memory/reference for Codex in this repository.
It records the available prompt commands and what each one does.

## Review Output Rule

- Any code review report must be saved under `docs/code-review`.
- Any story review report must be saved under `docs/story-review`.
- Review filenames must be prefixed with `codex-`.
- If it's the code review of a story, add story-{storyId} in the name.

## Key design files
- You may find those files under "docs/".
- Project context directly under it.
- Sprint and stories under "implementation-artifacts/".
- planning, design, brainstorms, etc... under "planning-artifacts/".

## BMAD agents available
- `bmad-agent-bmad-master`: primary BMAD orchestrator, expert in workflows and task execution.
- `bmad-agent-bmm-analyst`: business/product analyst specialized in scoping, requirements, and needs discovery.
- `bmad-agent-bmm-architect`: technical architect focused on system design and architecture decisions.
- `bmad-agent-bmm-dev`: senior software engineer responsible for technical story implementation.
- `bmad-agent-bmm-pm`: product manager guiding PRD creation and stakeholder alignment.
- `bmad-agent-bmm-qa`: QA engineer focused on rapid automated test generation.
- `bmad-agent-bmm-quick-flow-solo-dev`: fast full-stack developer optimized for small, quick-flow deliveries.
- `bmad-agent-bmm-sm`: technical scrum master preparing stories and facilitating sprint execution.
- `bmad-agent-bmm-tech-writer`: technical writer responsible for documentation quality and consistency.
- `bmad-agent-bmm-ux-designer`: UX/UI designer defining user flows, interactions, and interface specifications.
