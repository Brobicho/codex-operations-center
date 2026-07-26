# Codex Operations Center

An open-source, real-time operations center for Codex sessions across local projects.

The project combines truthful Codex lifecycle observability with an interactive terminal interface inspired by strategy and operations software. It does not invent agent personalities, progress percentages, or performance scores.

## Project goals

- Observe active Codex sessions across local projects.
- Translate raw lifecycle events into concise, human-readable activity.
- Surface approvals and failures that need attention.
- Provide an interactive mouse-and-keyboard control center.
- Render an advanced 3D scene in terminals supporting pixel graphics.
- Fall back cleanly to True Color Unicode and safe text modes.
- Install and uninstall without root access or project-local files.

## Planned rendering profiles

| Profile | Rendering | Intended environment |
| --- | --- | --- |
| Ultra | Off-screen 3D rasterization via the Kitty graphics protocol | Kitty, Ghostty, WezTerm, and compatible terminals |
| Unicode | True Color half-block software rendering | Modern terminals, tmux, and SSH |
| Safe | Accessible text-first control center | Minimal or unknown terminals |

## Data boundaries

- `codex app-server` is used to list stored local threads through its documented JSON-RPC API.
- Global Codex lifecycle hooks provide live events for every trusted local project.
- Raw transcript files are not treated as a stable API.
- Cloud and other-machine sessions require separate collectors and are not presented as locally observed activity.

## Commands

```bash
codex-ops                  # launch the operations center
codex-ops doctor           # inspect Codex and terminal capabilities
codex-ops integrate        # install global lifecycle hooks
codex-ops uninstall        # remove only the owned integration
codex-ops uninstall --purge
```

The integration writes its state beneath `~/.local/share/codex-ops` on Linux and adds one handler per supported event to `~/.codex/hooks.json`. Existing hooks are preserved. Codex requires users to review and trust newly installed hooks through `/hooks`.

## Status

Early development. Local thread discovery, event normalization, terminal capability detection, and reversible global-hook installation are implemented. The interactive renderer is under active development.

## License

MIT
