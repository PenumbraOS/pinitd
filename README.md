# Pinitd init system for Ai Pin

A custom, rootless init system for the Ai Pin to bypass access limitations.

> [!CAUTION]
> This is extremely experimental and currently is usable by developers only. See [Installation](#installation) for in-progress instructions on how to set it up.

`pinitd` is designed to expose a similar API surface to the `systemd` init system. When you install a service, you must enable it:

```bash
./data/local/tmp/bin/pinitd-cli enable [SERVICE_NAME]
```

Enabling it will make it eligable for automatic start and restart. However, the service is not yet started. To start it, run:

```bash
./data/local/tmp/bin/pinitd-cli start [SERVICE_NAME]
```

Besides providing a persistence mechanism, `pinitd` exploits [Zygote vulnerability CVE-2024-31317](https://github.com/agg23/cve-2024-31317/) in order to provide access to privileged user ids and SELinux domains. With it, you can spawn a process as any "app level" user (as defined in [`seapp_contexts`](https://android.googlesource.com/platform/system/sepolicy/+/refs/heads/master/private/seapp_contexts) (the Ai Pin has additional app contexts in `system_ext_seapp_contexts`)).

## CLI access

Control utility for the `pinitd` daemon. Installed into `/data/local/tmp/bin/pinitd-cli` by default.

Usage: `pinitd-cli <COMMAND>`

The `pinitd-cli` command provides the following subcommands:

| Command      | Description                                               |
| ------------ | --------------------------------------------------------- |
| `start`      | Start a service                                           |
| `stop`       | Stop a service                                            |
| `restart`    | Restart a service                                         |
| `enable`     | Enable a service (start on daemon boot if autostart=true) |
| `disable`    | Disable a service (prevent autostart)                     |
| `reload`     | Reload a service config from disk                         |
| `reload-all` | Reload all service configs from disk                      |
| `status`     | Show status of a specific service                         |
| `config`     | Show the current configuration of a service               |
| `list`       | List all known services and their status                  |
| `shutdown`   | Request the daemon to shut down gracefully                |
| `help`       | Print this message or the help of the given subcommand(s) |

## Config format

Like `systemd`, `pinitd` uses an ini unit file format. The available properties are:

| Property          | Required                        | Description                                                                                      | Format                                                |
| :---------------- | :------------------------------ | :----------------------------------------------------------------------------------------------- | :---------------------------------------------------- |
| `Name`            | Yes                             | A unique identifier for the service                                                              | String                                                |
| `Exec`            | Yes (if others are not present) | The command to execute                                                                           | String                                                |
| `ExecPackage`     | Yes (if others are not present) | Launches a binary embedded in an APK, looking up the package path automatically                  | `[PACKAGE]/[SUBPATH TO BINARY]` t`                    |
| `ExecJvmClass`    | Yes (if others are not present) | Launches a JVM process using `app_process`                                                       | `[PACKAGE]/[CLASS]`                                   |
| `ExecArgs`        | No                              | Extra arguments for `ExecPackage` or `ExecJvmClass`                                              | String                                                |
| `JvmArgs`         | No                              | Extra JVM arguments for `ExecJvmClass`                                                           | String                                                |
| `TriggerActivity` | No                              | An Android activity to trigger for exploit purposes. Defaults to `com.android.settings.Settings` | `[PACKAGE]/[ACTIVITY]`                                |
| `Uid`             | No                              | The user ID to run the service as                                                                | `System`, `Shell`, or a number. Defaults to `Shell`.  |
| `SeInfo`          | No                              | The SELinux context for the service                                                              | String                                                |
| `NiceName`        | No                              | A user-friendly name for the service. Only supported for `Uid=System`                            | String                                                |
| `Autostart`       | No                              | Whether the service should start on boot                                                         | `true` or `false`. Defaults to `false`.               |
| `Restart`         | No                              | The restart policy for the service                                                               | `Always`, `OnFailure`, or `None`. Defaults to `None`. |

## Example

A service that opens a port to allow opening a PTY into a privileged domain might look like this:

```ini
[Service]
Name=nfc-debug-service
Uid=1027
Exec=toybox nc -s 127.0.0.1 -p 1234 -L /system/bin/sh -l;
SeInfo=platform:nfc
```

## Installation

This is an active work in progress and may be difficult to set up. Please reach out to [@agg23](https://github.com/agg23) for questions or help.

1. Run `build.sh`. This will install the `pinitd-cli` binary and the Android APK that allows for autostart and contains the actual `pinitd` binary.
2. Due to https://github.com/PenumbraOS/pinitd/issues/4, starting apps may not work after setting up the `pinitd` environment. Start your primary app now to ensure it runs.
3. Start `pinitd`. At the time of writing this is accomplished by running:

```bash
settings delete global hidden_api_blacklist_exemptions && am force-stop com.android.settings
am start -n com.penumbraos.pinitd/.DummyActivity
```

but this will change in the future.

4. Once `pinitd` is running, if you had any autostart services, they should also now be running. Otherwise, you can manually start your services using:

```bash
./data/local/tmp/bin/pinitd-cli start [SERVICE_NAME]
```

## Troubleshooting

Due to `pinitd` relying on [Zygote vulnerability CVE-2024-31317](https://github.com/agg23/cve-2024-31317/), which involves a race on Android 12+, the vulnerability may fail randomly. Generally a failure will cause Zygote to reset the system (but not reboot), which may or may not be what we desire. In general, on any failure I recommend a reboot of the sytem.

### Rebooting

A bad application of the vulnerability payload or the system just being tempermental can completely stop Zygote from operating, which effectively soft bricks your system. Luckily `adbd` is not a normal Android process and will come back up if Zygote is freaking out, but you may have to time it correctly.

To avoid this scenario, always run `settings delete global hidden_api_blacklist_exemptions` whenever you're not actively exploiting Zygote, particularly if you're going to reboot. `pinitd` will try to do this automatically, but it sometimes fails.

### Boot looping

If you are stuck in a boot loop, don't panic. You want to spam call `adb shell settings delete global hidden_api_blacklist_exemptions`. It may take a number of executions to actually take effect, but once it does you should be free of the loop.
