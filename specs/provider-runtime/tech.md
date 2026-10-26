# Provider runtime integration

## Existing boundaries

The public repository contains the Rust client. Its built-in agent request path is
`app/src/ai/agent/api/impl.rs::generate_multi_agent_output`: it creates a multi-agent request
with API keys and custom model-provider settings and passes it to `warp_multi_agent_client`.
Changing the custom endpoint list alone does not replace this agent loop.

External runtimes are selected in `app/src/ai/agent_sdk/driver/harness/mod.rs`. ACP implementations
live in the adjacent `acp/` directory. They map session events to the client conversation model
and route prompts and cancellations through
`app/src/ai/blocklist/controller/external_harness.rs`.

The existing `Harness` enum in `crates/warp_cli/src/agent.rs` already includes OpenCode. Upstream
allows it for local child-agent terminal launches but rejects it for regular agent runs. The
initial patch fills this integration gap.

## Initial patch

- Route OpenCode through ACP regardless of the generic transport default, as Pi already does.
- Mark OpenCode conversations as native on both transport values so queue/cancellation routing
  follows the effective runtime.
- Resolve the built-in launch command to `opencode acp` without requiring a sidecar manifest.
- Identify the runtime with `CLIAgent::OpenCode` and provide installation documentation.
- Remove the regular-run rejection of OpenCode.
- Reuse ACP model discovery and `session/set_config_option`; a requested model absent from the
  runtime's advertised choices fails explicitly.
- Leave OpenCode provider configuration and credentials in OpenCode's own supported store.

OpenCode 1.19.4 was installed in the task's temporary tools directory for compatibility testing.
An isolated credential-free process completed ACP v1 `initialize` and `session/new` and returned
a `model` select setting. No paid model turn was sent by that handshake check.

A second real OpenCode process selected a custom model through `session/set_config_option`,
called a local OpenAI-compatible HTTP fixture with a dummy key, streamed the fixture's response
through `session/update`, and completed with `end_turn`. This verifies protocol compatibility;
it is not a live OpenAI, Anthropic, Z.ai, or native Warp GUI test.

## Try after building the fork

Install OpenCode according to its official instructions and connect a provider from its normal
interactive UI with `/connect`, then choose a model with `/models`. Use provider-supported
authentication; Claude subscription credentials must not be reused in OpenCode.

With this fork's CLI on PATH:

```text
oz agent run --harness opencode --prompt "Explain this repository without changing files"
```

To select a model, use a provider/model ID returned by OpenCode. Model IDs and access must be
discovered from the connected runtime rather than hard-coded in Warp.

This initial driver still follows Warp's task/conversation service setup. It is not a standalone
offline agent service. It also inherits the ACP driver's unattended approval semantics, so it
must not be presented as the final attended desktop replacement.

## Validation

The added Rust coverage checks that OpenCode resolves to a native ACP harness with the default
transport and that manifest discovery selects `opencode acp` without a sidecar. Existing ACP
mapping, model-selection, permission, and cancellation tests remain relevant.

`cargo test --locked -p warp --lib opencode_ -- --nocapture` passed: 12 tests, 0 failures.
The test build used the repository's Rust 1.92.0 toolchain on Windows, with Protocol Buffers
36.2 supplied as a local build tool.

`cargo clippy --locked -p warp -p warp_cli --all-targets --tests -- -D warnings` passed.
Final formatting passed for both modified packages using the repository's import-format
configuration, followed by `git diff --check`. Tests and lint were not rerun after formatting,
as specified by `AGENTS.md`.

## Desktop local runtime launcher

`LocalAgentRuntime` in `app/src/settings/ai.rs` identifies the three supported local CLI
runtimes. `AISettings.local_agent_runtime` persists the selection at
`agents.third_party.local_agent_runtime`, with `SyncToCloud::Never`. The default is OpenCode.
This preference applies to **New local agent tab**, not the existing Warp Agent tab action.

`CLIAgentsPageView` exposes a dropdown, a launch action, and runtime-specific setup guidance.
Runtime and provider-connection rows are separate searchable widgets, with a page-level title.
Local-launch widgets are only constructed for builds with `local_tty`.

`WorkspaceAction::AddLocalAgentTab` is registered in the command palette and new-session menu.
It creates a new terminal with `DefaultSessionModeBehavior::Ignore` and uses the existing
pending-command queue, so the bare `opencode`, `codex`, or `claude` command is submitted after
shell bootstrap. No prompts, credentials, interpolated arguments, or permission-bypass switches
are added. Missing CLIs fail visibly in the terminal; the action does not fall back to Warp.

The desktop launcher uses each CLI's own attended permission UI. It does not route through
the unattended ACP driver. Native rendering through ACP remains experimental as described above.

Desktop-launcher validation on Windows:

- `cargo test --locked -p warp --lib local_agent_ -- --nocapture`: 54 passed, 0 failed.
- `cargo test --locked -p warp --lib test_unified_new_session_menu`: 2 passed, 0 failed.
- `cargo clippy --locked -p warp --all-targets --tests -- -D warnings`: passed.

The new tests cover runtime-dependent pending commands in fresh tabs, preservation of existing
input, launcher availability with Warp AI disabled, a machine-local preference, and settings
search isolation with its page title retained. They use WarpUI's in-process test harness and
mocked terminal resources. A live desktop shell launch, visual layout, and authenticated
provider sessions still require end-to-end verification.

## Next integration boundary

Add a provider/runtime selection model above `HarnessKind` without making model-provider names
new harness variants. Bind each conversation to runtime identity, native session identity,
provider/model ID, and capability metadata. Runtime adapters own authentication and protocol
translation; the terminal owns conversation presentation and attended approvals.

A local-only session implementation would additionally need to replace server-owned task IDs,
conversation setup/persistence, and shared-session assumptions. Keep cloud sharing optional in
that implementation, and verify that no inference credentials or local transcript are sent to
Warp services unless the user has selected a feature requiring that transmission.
