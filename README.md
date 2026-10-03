# yatui

TUI and CLI for watching anime from YummyAnime via the Kodik player in `mpv`.

## Requirements

- Rust (edition 2024)
- `mpv` in `PATH`

## Build

```sh
cargo build --release
```

## TUI

```sh
yatui
```

| Screen | Keys |
| --- | --- |
| Search | type, `Enter` search, `Esc` to results, `Tab` library |
| Results | `Enter` open, `/` new search, `Tab` library |
| Title | `Enter` episodes, `1`–`5` set list, `x` remove from lists |
| Episodes | `Enter` choose quality, `w` toggle watched |
| Quality | `Enter` play |
| Library | `←`/`→` switch list, `Enter` open |

`j`/`k`, arrows, `PgUp`/`PgDn`, `g`/`G` navigate; `Esc` goes back; `q` quits.

Network requests run outside the UI thread. During loading, `Esc` cancels waiting and `q` or `Ctrl-C` quits. Requests have a 5-second connection timeout and a 20-second total timeout. Canceling discards the result; the in-flight request finishes in the background.

## CLI

```sh
yatui search "Frieren"
yatui info "Frieren" -n 1
yatui play "Frieren" -e 1 -t anilibria -q 720
yatui play "Frieren" -e 1 --print-cmd
yatui list [watching|planned|on-hold|dropped|completed]
yatui status <slug> planned
yatui status <slug>
```

`play` without `-e` picks the next unwatched episode and reuses the last translation. After mpv exits successfully, the episode is marked as watched.

## Lists

Local lists: Watching, Plan to Watch, On Hold, Dropped, Completed. Stored in `$XDG_DATA_HOME/yatui/library.json`.

- Watching an episode moves a title from Plan to Watch, On Hold or Dropped to Watching.
- Watching the last episode moves it to Completed.

## Subtitles

If Kodik returns external subtitle files, they are passed to mpv via `--sub-file`. Kodik's "Субтитры" translations currently ship subtitles burned into the video.

## Tokens

By default requests use the built-in public application token. Stored in `$XDG_CONFIG_HOME/yatui/config.toml` (mode `0600`):

```sh
yatui token set <USER_TOKEN>   # private user token, sent as `Authorization: Yummy <token>`
yatui token app <APP_TOKEN>    # own application token from https://yummyani.me/dev/applications
yatui token status             # show used tokens, verify the private one via /profile
yatui token refresh            # renew the private token via /profile/token (valid for 2 weeks)
yatui token clear
```

`YATUI_USER_TOKEN` and `YATUI_APP_TOKEN` override the saved values.
