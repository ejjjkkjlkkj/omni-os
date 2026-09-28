# Agent execution contract

The agent now has an executable local tool layer. Normal runs remain observation-only. Use `--verify` to execute the declared verification tools.

Pipeline:
inspect -> understand -> select tools -> execute under policy -> collect evidence -> update state -> next task.

A tool result is PASS only when the process exits successfully. Missing tools, timeouts, policy violations and unavailable prerequisites remain UNKNOWN or BLOCKED. No result is converted into PASS by inference.

Security and accessibility are independent verification dimensions. The canonical knowledge base and required security source remain blockers when unavailable.

Writes outside protected state are not enabled by the verification loop. Any future write capability must remain policy-gated and checkpointed.
