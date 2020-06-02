# Changelog

## v0.2.5 (June 3, 2020)

### Fixes
- Fixed duplicate db_connections_total metric ([181b66b](https://github.com/netology-group/event/commit/181b66bbf49f520d03abc79d718b4c0ef4be6d17))

### Changes
- Disabled room presence check for event creation ([1b9467d](https://github.com/netology-group/event/commit/1b9467daaa52bed52f1b80768edbc5055445407d))

## v0.2.4 (May 27, 2020)

### Changes
- Added resubscription on reconnect ([1609031](https://github.com/netology-group/event/commit/160903112414c18936740f67ffaf6b54d66ddedf))
- Made a switch from anyhow to failure ([6487dd0](https://github.com/netology-group/event/commit/6487dd0bc28c59d0f47a6603bda525e38997399b))
- Moved to new version of svc-agent ([abec10b](https://github.com/netology-group/event/commit/abec10b52e2ef7318ce057f102f49a59feca0b8b))
- Added queues length metrics ([11003a8](https://github.com/netology-group/event/commit/11003a8fdb46194f1b42c745e1ffb86af37fb7c5))

## v0.2.3 (May 18, 2020)

### Changes
- Switch runtime to smol ([56625aa](https://github.com/netology-group/event/commit/56625aa29c8ce60ba83a7a30eb784df9b14c833c))
- Optimize DB connection usage ([6c672a7](https://github.com/netology-group/event/commit/6c672a7a6deb6aca70ca88f7ee15d77130e1d931))


## v0.2.2 (May 15, 2020)

### Features
- Telemetry metrics handler, Kruonis subscription ([399d94b](https://github.com/netology-group/event/commit/399d94b4754fef5a9643af28de89578012d2f058), [d790943](https://github.com/netology-group/event/commit/d790943a3896fda593ad4008e54ca99f068ded57), [a73374e](https://github.com/netology-group/event/commit/a73374ee6500db00dda66ce5e0d8fd352b202e20), [ba175b9](https://github.com/netology-group/event/commit/ba175b9caf6476d101c4ee4d4c5d4ffc786ecacf), [299080d](https://github.com/netology-group/event/commit/299080d4cec1abac21c7c4494c1e0c407df226ec), [9487763](https://github.com/netology-group/event/commit/94877637d1d424b09512b24fcaf61b9bc64392f1), [98b791d](https://github.com/netology-group/event/commit/98b791dcb7ffef97e49de73037d454bdd50188d5))

### Changes
- Rename `event_kind` ([a09a213](https://github.com/netology-group/event/commit/a09a213de1b69651fee71b084126f6141366a9de))
- Expose `type`, `set` and `occurred_at` on `RemovalData` ([936a884](https://github.com/netology-group/event/commit/936a884798ea5a83563d35c8afe850f4b3f3573b))


### Fixes
- Use cut changes in edition commit modified segments ([6e81dfd](https://github.com/netology-group/event/commit/6e81dfd4300b200fe1091c99f6707ff90d418e06))
- Fix `change.event_created_by` not being applied to edition commit ([cb23ae9](https://github.com/netology-group/event/commit/cb23ae9ea1b6ef0ac5bc334f0e2dfcad41851ac6))


## v0.2.1 (April 21, 2020)

### Changes
- svc-agent was updated to v0.12.0 ([c775ffd](https://github.com/netology-group/event/commit/c775ffd6b6cc3677c905cdbd090a7983dd87d7fc))

### Fixes
- `edition.delete` was added to routes ([0d9a43f](https://github.com/netology-group/event/commit/0d9a43f3b10bae16c02250f444a761110ea9609c))

## v0.2.0 (April 14, 2020)

### Breaking changes
- Split occurred_at & original_occurred_at in state.read ([5f5d8aa](https://github.com/netology-group/event/commit/5f5d8aa9d467b38da02f483a94c9c21daee329ed))

### Features
- Add editions API ([1f67466](https://github.com/netology-group/event/commit/1f6746608430a8218fb6d5e5420bcb6fd99d0ed6), [e1740d7]( https://github.com/netology-group/event/commit/e1740d7d840c9e161e1cd8227f37a7f954587243))
- Add `room.update` ([70e7374](https://github.com/netology-group/event/commit/70e7374b3943e230172540569a25ddc929fe9151))

### Changes
- Upgrade svc-authz ([f2b1640](https://github.com/netology-group/event/commit/f2b164023dc787167597c7035ed2d4d6aece73cc))
- Allow updating room tags on closed rooms ([ab38383](https://github.com/netology-group/event/commit/ab38383b1edc16bd588f0672cfc204527db32322))

### Fixes
- Fix `room.adjust` algorithm ([36593a4](https://github.com/netology-group/event/commit/36593a47311395a77486b9697575022cae2a2845), [6ec76c0](https://github.com/netology-group/event/commit/6ec76c0b6bef66e333ba41da0836c391f71dc93d), [71127b3](https://github.com/netology-group/event/commit/71127b33cfbfe6f9ece9df21c2bf6d51456dc94d), [c7ccf21](https://github.com/netology-group/event/commit/c7ccf21a071aefc3e41da083c3643acf2a8e5eeb), [28d058e](https://github.com/netology-group/event/commit/28d058e048ca0a2c6657d5bbe5f84f00ccb7c4ba))

## v0.1.1 (March 26, 2020)

### Features
- Add `edition.delete` API method ([398200b](https://github.com/netology-group/event/commit/398200bfbbea8905bff60fa256245c2a3d46ea55)).

### Changes
- Upgrade svc-authz ([e31f744](https://github.com/netology-group/event/commit/e31f7445bcf6eff731ab2e0b058a88b6560b49c7)).


## v0.1.0 (March 20, 2020)

Initial release
