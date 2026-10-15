# Configuration reference

Since F12, Crimson WM reads an optional TOML file, `crimson/wm.toml`. This
page is the reference for its format and behaviour. The design decisions
are recorded in [ADR 0014](adr/0014-configuracion-toml.md) (Spanish), and
[examples/wm.toml](examples/wm.toml) is a commented file that reproduces the
built-in defaults exactly.

> [!NOTE]
> Crimson WM is pre-alpha and only starts on test X servers that pass its
> safeguards ([ADR 0003](adr/0003-salvaguardas-desarrollo.md)). The
> configuration cannot change those safeguards.

## At a glance

- The file is read **once, at startup, before connecting to X**. There is
  **no live reload**: restart the window manager to apply changes.
- The whole file is validated before anything is applied. **An invalid file
  stops the window manager with exit code 15**, with the file, line and
  column of the problem. Nothing is ever applied partially.
- **Without a file, the defaults are exactly the behaviour of F11.**
- Only four sections exist: `[layout]`, `[programs.NAME]`, `[[keybindings]]`
  and `[[rules]]`. Any other key or table is an error.
- `crimson-wm --check-config [PATH]` validates a file without any X server.

## Where the file is looked for

| Order | Source | When it is used |
|---|---|---|
| 1 | `--config PATH` | Only that file is used. It must exist; nothing else is searched. |
| 2 | `$XDG_CONFIG_HOME/crimson/wm.toml` | If `XDG_CONFIG_HOME` is set, not empty and an absolute path. |
| 3 | `$HOME/.config/crimson/wm.toml` | If `HOME` is set, not empty and an absolute path. |
| 4 | Built-in defaults | If none of the files above exists. |

- `XDG_CONFIG_HOME` **unset or empty**: only the `HOME` location is tried,
  without any message.
- `XDG_CONFIG_HOME` **relative** (for example `config`): it is invalid under
  the XDG Base Directory specification, so it is ignored with a warning in
  the log, and the `HOME` location is tried.
- If both variables give the same path, it is tried once.
- The first location **that exists** is used. If that file exists but is
  unreadable or invalid, the window manager stops with code 15: it never
  falls back silently to the next location or to the defaults.
- A dangling symbolic link counts as existing (it is an error, not a missing
  file).
- No file or directory is ever created.

The log states which source was used, for example
`configuración: /home/user/.config/crimson/wm.toml (30 atajos, 6 programas,
maestra al 60 %, 0 reglas)`, or `configuración: predeterminada (no existe …)`.

## File requirements

The file is checked on the opened file descriptor:

- it must be a **regular file**; symbolic links are followed and the target
  is checked (directories, FIFOs, sockets and devices are rejected, FIFOs
  before opening them; a nonblocking open also prevents a FIFO substituted
  between the first check and open from blocking startup);
- it **must not be writable by group or others** (`chmod go-w`), because it
  decides which programs run with your permissions;
- at most **64 KiB** (65536 bytes);
- valid **UTF-8**.

The opened descriptor must refer to the same device and inode as the first
check. File ownership and parent-directory permissions are not checked:
keep the configuration in directories you control. These checks do not
provide an atomic snapshot if another process writes to the same inode.

Error messages may quote TOML keys, key and modifier names, action, type
and program names (shortened to 32 characters) and numbers, but never the
contents of the file otherwise: program arguments and the `instance` and
`class` strings of rules are never echoed.

## Syntax

Standard TOML 1.1. Tables can be written with headers, dotted keys or inline
tables; all forms are equivalent. Dates and floating-point numbers are
valid TOML but are never valid values here.

### `[layout]`

| Key | Type | Range | Default |
|---|---|---|---|
| `default` | string | `"master-stack"` or `"columns"` | `"master-stack"` |
| `master_percent` | integer | 10–90 | 50 |

Width of the master column, in percent of the work area. With 1280 pixels
and `master_percent = 60`, the master column is 768 pixels wide and the
stack 512. A single tiled window always fills the work area.

Since F13, each of the eight desktops starts with `default` and keeps its
own layout in memory. Master-stack is unchanged from F12. Columns divides
the work area's width into equal columns; leftover pixels go to the first
windows (1280 pixels / 3 gives 427, 427, 426). Both layouts exclude floating
and fullscreen windows. Fullscreen uses the physical screen; leaving it
uses the desktop's current layout.

`master_percent` remains global, is always validated, and is retained while
using Columns. It takes effect again when switching back to Master-stack.
Changing layouts does not change focus, window order, rules, floating
geometry or desktop membership. Selections survive empty desktops and
desktop switches, but are not saved across restarts. Very narrow areas may
require overlapping one-pixel columns; extreme coordinates saturate as in
F12. See [ADR 0015](adr/0015-motor-de-layouts.md).

An optional binding (there is **no new default binding**) is:

```toml
[[keybindings]]
keys = "Mod4+space"
action = "cycle_layout"
```

This must be part of a complete binding table including an `exit` binding:
explicit tables still replace all default bindings. The action cycles
Master-stack → Columns → Master-stack only on the active desktop and
ignores server autorepeat. Its `keys` name is `space`, not `Space`.

### `[programs.NAME]`

```toml
[programs.terminal]
argv = ["kitty"]

[programs.menu]
argv = ["rofi", "-show", "drun"]
```

- `NAME`: a lowercase ASCII letter, then lowercase letters, digits, `_` or
  `-`; 32 characters at most. At most 64 programs.
- `argv`: the only key; a list of 1 to 32 strings, each at most 4096 bytes,
  without NUL characters.
- `argv[0]` is either a plain name, looked up in the window manager's
  `PATH`, or an absolute path. It cannot be empty, a relative path with `/`,
  start with `~` or contain `$`.
- **No shell**: there are no pipes, redirections, `~` expansion or
  `$VARIABLES`; every argument is passed literally (`"$HOME"` arrives as the
  five characters `$HOME`). There are no keys for the environment, working
  directory or user: programs inherit the window manager's environment with
  the same sanitising as F4 (the sandbox token, `WINDOWID`,
  `DESKTOP_STARTUP_ID` and `CRIMSON_RULES` are removed and `DISPLAY` is set
  to the validated display).
- **Defining `[programs]` replaces the whole default table** (no merging).
  If the file has no `[[keybindings]]`, the default launch bindings look up
  their program by name (`terminal`, `lanzador`, `archivos`, `navegador`,
  `editor`, `capturas`); a missing one is an error.

### `[[keybindings]]`

```toml
[[keybindings]]
keys = "Mod4+Shift+Escape"
action = "exit"

[[keybindings]]
keys = "Mod4+Return"
action = "launch"
program = "terminal"

[[keybindings]]
keys = "Mod4+Shift+3"
action = "move_to_desktop"
desktop = 3
```

**`keys`**: modifiers joined with `+`, then exactly one key, without spaces.
The order of the modifiers does not matter; bindings are compared after
normalising them, so `Shift+Super+j` and `Mod4+Shift+j` are the same.

| Modifier | Aliases | X11 mask |
|---|---|---|
| `Shift` | — | Shift |
| `Control` | `Ctrl` | Control |
| `Mod1` | `Alt` | Mod1 |
| `Mod4` | `Super` | Mod4 |

`Alt` and `Super` assume Debian's usual modifier map (Alt_L on Mod1,
Super_L on Mod4). `Lock`, `Mod2` (Num Lock), `Mod3`, `Mod5`, `Hyper`,
`Meta` and `NumLock` are rejected; Caps Lock and Num Lock are always ignored
when matching a key press.

Key names are X11 keysym names and are case-sensitive:

- letters `a`–`z` (lowercase; for a capital, add `Shift`) and digits `0`–`9`;
- `Return`, `Escape`, `Tab`, `BackSpace`, `Delete`, `Insert`, `Home`, `End`,
  `Prior` (alias `Page_Up`), `Next` (alias `Page_Down`), `Left`, `Up`,
  `Right`, `Down`, `space`;
- `minus`, `equal`, `comma`, `period`, `slash`, `semicolon`, `apostrophe`,
  `grave`, `bracketleft`, `bracketright`, `backslash`;
- `F1`–`F12`, `Print`;
- `XF86AudioLowerVolume`, `XF86AudioRaiseVolume`, `XF86AudioMute`,
  `XF86AudioMicMute`, `XF86AudioPlay`, `XF86AudioStop`, `XF86AudioPrev`,
  `XF86AudioNext`, `XF86MonBrightnessUp`, `XF86MonBrightnessDown`.

**Typing keys** (letters, digits, punctuation, `space`, `Return`, `Tab`,
`Escape`, editing and arrow keys) need `Control`, `Mod1` or `Mod4`, so that
a binding cannot capture normal typing. Function keys, `Print` and media
keys may be bound alone.

**`action`** and its parameters:

| Action | Parameter | Effect |
|---|---|---|
| `exit` | — | Quit (exit code 0) |
| `focus_next`, `focus_prev` | — | Focus the next / previous window |
| `zoom` | — | Make the focused window the master (or swap it with the second) |
| `cycle_layout` | — | Cycle the active desktop's layout without changing focus or window order |
| `launch` | `program = "NAME"` | Start that program from `[programs]` |
| `view_desktop` | `desktop = 1…8` | Show that desktop |
| `move_to_desktop` | `desktop = 1…8` | Send the focused window there |
| `view_previous` | — | Go back to the previous desktop |
| `close` | — | Ask the focused window to close |
| `kill` | — | Force-close the focused client |
| `fullscreen` | — | Toggle fullscreen |

Rules for the table:

- **Defining `[[keybindings]]` replaces the whole default table.** It must
  contain **at least one `exit` binding**; there can be more than one.
- At most 128 bindings. A repeated binding (after normalisation), an
  unknown key, modifier or action, a missing or unused parameter (for
  example `desktop` on `close`) or an undefined program is an error.
- At startup, control bindings are mandatory: if one cannot be grabbed
  (no keycode produces its key, another client holds it, or two bindings
  end up on the same keycode), the window manager stops with code 14.
  Launch bindings are optional: they are disabled with a warning.
- A binding cannot use a modifier assigned to Num Lock on the current
  server: that modifier is ignored when matching events. Such a control
  binding stops startup with code 14; a launch binding is disabled with a
  warning. The usual Num Lock mapping on Mod2 does not conflict with any
  supported binding modifier.

### `[[rules]]`

Window rules with the semantics of F11
([ADR 0013](adr/0013-reglas-de-ventanas.md)):

```toml
[[rules]]
class = "Firefox"
desktop = 2

[[rules]]
instance = "pavucontrol"
floating = true
nofocus = true
```

| Key | Type | Meaning |
|---|---|---|
| `instance`, `class` | string, 1–256 bytes | Exact, case-sensitive match on the two `WM_CLASS` strings |
| `type` | string | Effective EWMH type: `normal`, `dialog`, `utility`, `toolbar`, `splash`, `menu`, `dropdown_menu`, `popup_menu`, `tooltip`, `combo`, `dnd` |
| `transient` | boolean | Has (`true`) or lacks (`false`) `WM_TRANSIENT_FOR` |
| `desktop` | integer 1–8 | Initial desktop; the window waits hidden there |
| `floating`, `nofocus`, `fullscreen` | boolean | `true` applies the action; `false` is the same as omitting it |

- Every rule needs at least one condition and one action; at most 64 rules.
- All rules are evaluated in order; each action is set by the first matching
  rule that defines it. Logs refer to rules by the line of their `[[rules]]`
  header.
- As in F11, rules never apply to windows adopted at startup,
  override-redirect windows, docks, notifications or desktop windows, and a
  dialog with an owner stays on its owner's desktop.

### `CRIMSON_RULES` (deprecated)

The F11 environment variable still works during F12, with this precedence:

- **If `CRIMSON_RULES` is set, it replaces the rules of the file entirely**
  (no merging), and the log shows a deprecation warning and, if the file had
  rules, that they are not applied. An empty value also replaces them (no
  rules).
- **If it is set but invalid**, it is discarded as a whole, exactly as in
  F11 (`CRIMSON_RULES inválida (…): se continúa sin reglas`), and the window
  manager starts **without rules**: the file's rules are not used either.
- The file is always validated completely first: an invalid `[[rules]]`
  stops the window manager even if `CRIMSON_RULES` would replace it.

## `--check-config`

```sh
crimson-wm --check-config                 # the file that startup would use
crimson-wm --check-config ~/wm-test.toml  # a specific file
```

- Uses the same search order, file checks and validation as startup.
- Does **not** read `DISPLAY`, `XAUTHORITY` or the sandbox token, does not
  connect to any X server, does not launch anything and does not write any
  file. It works without an X server.
- Prints the source that was validated and a summary on standard output,
  plus any warnings (for example about `CRIMSON_RULES`), and exits with **0**
  if the configuration is valid or **15** if it is not (the error goes to
  standard error).

Command-line syntax:

```text
crimson-wm [--config PATH]
crimson-wm --check-config [PATH]
crimson-wm --help | --version
```

`PATH` is a separate argument (`--config=PATH` is not accepted) and cannot
start with `-`; write `./-file` for such a file. Any other argument is a
usage error (exit code 2).

## Exit codes

Code 15 extends the table of [ADR 0003](adr/0003-salvaguardas-desarrollo.md):

| Code | Meaning |
|---|---|
| 0 | Normal exit; or `--check-config` with a valid configuration |
| 2 | Invalid arguments |
| 10 | Rejected by the safeguards (display 0, non-local display, sandbox marker) |
| 11 | Display unreachable |
| 12 | Another window manager is running |
| 13 | Connection to the X server lost |
| 14 | Unexpected protocol error at startup (including a control binding that cannot be grabbed) |
| 15 | Unreadable or invalid configuration (detected before connecting to X) |

The safeguards come first: with `DISPLAY=:0`, the window manager exits with
10 even if the configuration is invalid.

## Not configurable

These are deliberately fixed:

- the safeguards (display `:0`, sandbox token and marker, other window
  manager check) and the sanitising of the programs' environment;
- the number of desktops (8);
- ICCCM/EWMH policies (window types, layers, dialogs, fullscreen rules);
- internal limits (property sizes, launch supervision, key repeat filter).

## Not implemented yet

- Live reload (it needs an IPC channel or a signal handler; see ADR 0014).
- Borders, gaps, colours, mouse settings, focus policies, other layouts,
  scratchpads and per-rule geometry: they do not exist in the window
  manager yet, so they have no keys.
