# Agent tools contract

The agent is a bounded local maintenance system, not a fixed checklist runner.

## Autonomous cycle

inspect -> understand -> prioritize -> checkpoint -> modify -> test -> security -> accessibility -> regression -> evidence -> next task

The planner may inspect the entire repository and construct a work queue. It should select the smallest useful change, verify it, and then continue with the next independent task.

## Engineering evidence

A task is complete only when the relevant evidence exists. A source document, file presence, or successful unrelated test does not prove a requirement. Requirements must remain traceable to implementation, tests, security evidence, accessibility evidence, and provenance.

## Write boundary

Automatic changes are checkpointed and non-destructive. The agent must never:
- rewrite Git history;
- push remotely;
- access credentials or secrets;
- delete recursively;
- modify protected state through generic write tools;
- claim success without verification.

Before a write, the agent must establish a recoverable checkpoint and a bounded target. After a write, it must run the narrowest relevant tests, then broader regression checks when practical.

## Zero-cost local operation

The core runtime uses the Windows host, Python standard library, Git, and toolchains already installed in the project. No API key, subscription, or cloud service is required.

A local model can be integrated later as an optional decision component, but the repository agent must remain functional without one.

## Quality target

The design follows practices used by modern software-engineering agents: repository navigation, targeted edits, test-driven repair, reproducible evidence, failure classification, and iterative repair. Benchmarking such behavior on real repository tasks is an established approach in SWE-bench.