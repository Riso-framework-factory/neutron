# 0007. Core is the dependency closure of declared roots

Status: accepted (2026-10-24). Supersedes the core batches B5 and B6 of the porting plan.

## Context
Phase 2 filled core with every upstream crate whose dependencies stayed inside core and that might
be useful to an app. That brought in 50 crates that no framework crate needs: language model
providers, the MCP client, agent skills, the language/LSP/terminal engines, markdown, node_runtime,
feature_flags, zed_actions and others. They belong to Zed's agentic IDE and contradict ADR 0006.

## Decision
`upstream.toml` lists `[core] roots`: the upstream crates the framework itself needs, grouped with
the reason. Core is their closure over normal and build dependencies, computed by
`cargo neutron sync`. Every other upstream crate stays in the `apps/ide` mirror.

A crate becomes a root only when `riso/` or an app builds on it. A crate that merely looks useful
stays in `apps/ide` until then.

## Consequences
- Core shrinks from 123 to 73 crates, and its dev-dependencies are closed too.
- Eight IDE-flavoured crates remain because verbatim roots depend on them:
  `settings_content` pulls language_model_core, cloud_llm_client and zeta_prompt; `fs` pulls git,
  askpass, net, proto and telemetry. Removing them would mean shimming settings_content and fs.
- When upstream gives a core crate a new dependency, that crate moves into core automatically and
  the sync report flags it for triage; crates no root needs any more leave core the same way.
- grammars leaves core, so debug builds read assets from the checkout again and the workspace no
  longer needs `util/debug-embed`.
