# Multi-provider Warp fork

## Goal

Keep Warp's terminal and conversation UI while allowing the user to choose the agent runtime,
model provider, and authentication method. Initial targets are Codex, Claude, and Z.ai, with
both existing agent logins and API keys where supported by the provider.

The project fork is https://github.com/goldflyfish/warp. Its initial base is upstream commit
`1e6c3ae465d6a925a9b97ae03877aaadd159b8f1`.

## Product model

Agent runtime, inference provider, and authentication are separate choices:

| Runtime | Provider | Authentication |
| --- | --- | --- |
| Codex | OpenAI or a runtime-supported endpoint | Existing Codex login or API key |
| Claude Code terminal | Anthropic | The user's own CLI login or API key |
| OpenCode | OpenAI | Supported ChatGPT login or API key |
| OpenCode | Anthropic | API key |
| OpenCode | Z.ai | API key; use the endpoint appropriate to the user's plan |
| OpenCode | Other supported or custom providers | Provider-supported credentials |

Codex is an agent runtime; OpenAI is its model provider. The same distinction applies to
Claude Code and Anthropic. A provider switch must not silently replace the user's agent runtime.

Anthropic's Agent SDK documentation says third-party products cannot offer claude.ai login or
subscription limits without prior approval. Retain the user's own interactive Claude Code
terminal workflow for login-based use. Use API authentication for an embedded Claude SDK
integration unless Anthropic approves another route. Do not extract Claude subscription tokens
and pass them to OpenCode or to a direct HTTP adapter.

## First implementation

OpenCode is the first multi-provider runtime, connected through the Agent Client Protocol (ACP).
This reuses Warp's existing native conversation rendering, cancellation, tool display, model
selection, and MCP transport. It avoids implementing a second tool loop for each model API.

The initial change makes `--harness opencode` resolve to `opencode acp`, enables it for ordinary
agent runs, and identifies it as OpenCode in the client. Existing Codex and Claude CLI paths
remain available. Provider credentials remain managed by the selected runtime.

This is an integration milestone. The ordinary desktop default is still Warp Agent. A unified
provider settings screen, default-runtime preference, and an end-to-end desktop verification
are still required before describing the fork as a replacement for Warp Agent.

## Desktop local runtime launcher

The desktop now has a separate **New local agent tab** command and a new-tab menu entry.
In **Settings > Agents > Third party CLI agents**, select OpenCode, Codex, or Claude Code.
OpenCode is the initial local preference; the choice is saved only on this computer.

The action opens a fresh terminal tab, inherits the ordinary new-tab working-directory rules,
waits for shell startup, and starts the chosen interactive CLI. It does not reuse the active
terminal's input or send a prompt to Warp's inference service. It remains available when Warp
AI is disabled. The existing Warp Agent and cloud session actions keep their existing meaning.

The runtime must already be installed in the selected shell. The settings page links to its
official setup guide and explains login/API-key support. Authentication, provider selection,
and permission prompts happen in the runtime's own UI. The launcher does not copy credentials,
edit global provider configuration, pass auto-approval flags, or silently select another agent.

This makes the runtimes accessible from the desktop, but it is an interactive terminal route.
The native ACP conversation bridge remains a separate integration. A native provider/model
selector, attended ACP approvals, connection status, and packaging are still needed for the
finished replacement experience.

## Required behavior for the finished product

1. A user can select a runtime and discover its models without editing source code.
2. A user can connect with an API key or the runtime's supported login flow. Unsupported
   combinations are explained in the UI.
3. Credentials are never committed or copied between unrelated runtimes. Connecting a runtime
   does not rewrite the user's global Codex or Claude configuration.
4. Provider failures remain visible. The module must not silently fall back to Warp credits or
   another paid provider.
5. New conversations use the selected default runtime. Existing conversations retain the
   runtime and model needed to resume them.
6. Tool progress, file changes, cancellation, errors, and approvals are visible in the terminal
   application, with capability differences shown where they affect a decision.
7. Adding a provider supported by OpenCode requires configuration, rather than a new Warp enum.
8. Windows PowerShell, bash, zsh, and fish behavior is verified before distributing installers.

## Work still required

- Add the runtime/provider/model selector to native desktop conversations and save a native default.
  The separate local terminal launcher already saves its preferred runtime.
- Add connection status, setup instructions, and supported login/API-key actions for each runtime.
- Replace the unattended ACP permission behavior with an attended approval flow for ordinary
  desktop conversations. The upstream ACP driver currently auto-approves requests unless its
  policy explicitly refuses them; it is not yet the finished interactive approval experience.
- Decide whether local-only sessions without a Warp account are required. Current task creation,
  conversation persistence, and sharing still involve Warp services. Replacing inference alone
  does not remove those dependencies.
- Verify real authenticated turns with OpenAI, Anthropic, and Z.ai, plus cancellation, file edits,
  denied actions, provider failure, and conversation resumption.
- Brand and package the fork after the integration is stable. Preserve upstream license notices.

## Sources checked on 2026-10-26

- [Warp source and license overview](https://github.com/warpdotdev/warp)
- [Codex app-server](https://learn.chatgpt.com/docs/app-server)
- [Claude Agent SDK and authentication restrictions](https://code.claude.com/docs/en/agent-sdk/overview)
- [OpenCode ACP](https://dev.opencode.ai/docs/acp/)
- [OpenCode providers](https://docs.opencode.ai/docs/providers/)
- [Z.ai quick start](https://docs.z.ai/guides/overview/quick-start)
