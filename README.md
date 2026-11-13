# Alarm Clock

Keith's overengineered alarm clock.

## Secrets

> [!CAUTION]
> Remember to `chmod u=rwx,go= secrets/*`.

Secrets live under a `secrets/` directory in the root of this project. Required are:

Filename | Contents
--- | ---
`wifi_ssid.txt` | The WiFi SSID with no whitespace.
`wifi_password.txt` | The WiFi password with no whitespace.
`calendar_secrets.json` | JSON secret file of a GCP OAuth 2.0 Client ID with Calendar API scope.

Also required is `calendar_refresh_token.txt`, but that can be created using:

```sh
$ cd refresh_token_generator
$ cargo run
Go to https://www.google.com/device and enter XXX-XXX-XXX to authorise...
Refresh token is: ...
```

## Audio Files

Under `alarm_clock/alarm_sounds`, and embedded into the firmware image in `alarm_clock/src/main.rs`.

Convert from MP3 to WAV using:

```sh
$ nix run nixpkgs#ffmpeg -- -i alarm_clock/alarm_sounds/input.mp3 -acodec pcm_s16le -ac 1 -ar 16000 alarm_clock/alarm_sounds/output.wav
```