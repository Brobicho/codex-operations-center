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

## Status

Early development. The command-line bootstrap and release infrastructure are being built first, followed by Codex event collection and the interactive renderer.

## License

MIT

