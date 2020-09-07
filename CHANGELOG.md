# Changelog

## v0.2.15 (September 7, 2020)

### Fixes
- Update svc-agent ([e165376](https://github.com/netology-group/event/commit/e165376c72dc0f2f3afe3ccfa45ca70b279fee99))


## v0.2.14 (September 7, 2020)

### Changes
- Update svc-agent ([94c2593](https://github.com/netology-group/event/commit/94c2593eebf3aa0f266d90dadcc46ccf21e524dc), [911ce6e](911ce6eaec47c7a53c030e89f1915217972b280b))
- Update svc-error ([4ea199d](https://github.com/netology-group/event/commit/4ea199d14c57eb95655f0b8e95314fba73ec7888))
- Execute DB queries in th blocking thread pool ([cbd0c13](https://github.com/netology-group/event/commit/cbd0c13acc4994b95708993cc541f57952a226ce))
- Pass lifetime setting to DB connection pool ([2abd0d5](https://github.com/netology-group/event/commit/2abd0d597160f2abb8e45a2f689c6c517d77dd9a))
- Log request handler errors ([7dfbd07](https://github.com/netology-group/event/commit/7dfbd07df388c7c9bc3ffcbbd9ef96434a4375f8))


## v0.2.13 (August 31, 2020)

### Features
- Add readonly replica DB connection pool ([4f040ea](https://github.com/netology-group/event/commit/4f040ea0c0d4cddfa2ee843f744e40f4d868287a))
- Add query time profiling ([f193fbb](https://github.com/netology-group/event/commit/f193fbb846674e01f276a6eb5460b797a39e473c), [c3bac8c](https://github.com/netology-group/event/commit/c3bac8ce965bd89e04023473aa1342f698f4c2ea))
- Add avg and max checkin and checkout metrics for db pools ([a146389](https://github.com/netology-group/event/commit/a1463893814443c1f099eab56fff9c72c9069bc6), [6260794](https://github.com/netology-group/event/commit/6260794a0683bbeb7ae7485d317efe5d13ae05a0))


## v0.2.12 (August 28, 2020)

### Features
- Allow event list filtering by type or multiple types ([8328c61](https://github.com/netology-group/event/commit/8328c61a319ec48b6970d5af41a200749cd382d7))


## v0.2.11 (August 27, 2020)

### Features
- Add CACHE_ENABLED envvar check ([9fd18b0](https://github.com/netology-group/event/commit/9fd18b05b6e10e2595da19bddfe1b8e9bbd7efb7))
- Transmit idle connections metrics both for redis and pg pools ([47c2cd1](https://github.com/netology-group/event/commit/47c2cd1da6d428050b296010fbecbff2b11974e0))

### Fixes
- Subscribe to unicast requests without shared group ([9c9005a](https://github.com/netology-group/event/commit/9c9005a7a8938afc7419ec511be19c54ea849e72))


## v0.2.10 (July 30, 2020)

### Fixes
- Moved MessageHandler `handle()` invocation to separate blocking executor ([ab39474](https://github.com/netology-group/event/commit/ab39474df5439b413f48f7fb8dd59ab3f409295a))

### Changes
- Added unix signals handlers ([675a2ef](https://github.com/netology-group/event/commit/675a2ef9f85747244efc6d715fdb8af8259647fb))
- Svc agent update ([06ce6df](https://github.com/netology-group/event/commit/06ce6df11c2af48071089819b82f0526ec90d526))

## v0.2.9 (July 21, 2020)

### Changes
- Significantly improved commit edition query ([d68585a](https://github.com/netology-group/event/commit/d68585a43784bd63c71fee958ce9ce4a5519b505))

### Fixes
-  Moved blocking call to separate futures thread pool ([f18ae19](https://github.com/netology-group/event/commit/f18ae198ef43d9307a3406f7eba535a61ae5c5c9))

### Features
-  Added `redis_connections_total` metric ([dba89bc](https://github.com/netology-group/event/commit/dba89bc338266a4b264bda82d1e8e1da9ce42cb9))


## v0.2.8

## v0.2.7 (June 11, 2020)

### Changes
- Notification loop thread now has a name ([7f67611](https://github.com/netology-group/event/commit/7f676113dea31bbd186c5b11cdb3fd73e37cb38d))
- svc-agent update to fix rumq not handling max_packet_size properly ([9660af4](https://github.com/netology-group/event/commit/9660af4018f90a95e61c4c1b4354fde950e61d0f))

## v0.2.6 (June 4, 2020)

### Changes
- Upgrade async-std ([663b129](https://github.com/netology-group/event/commit/663b12903e347f87a0517809d6e4eca15ef78829))
- Upgrade svc-agent ([4cb0db8](https://github.com/netology-group/event/commit/4cb0db8c95c86a95697e9c17205f7fe44be24423), [dd4fee8](https://github.com/netology-group/event/commit/dd4fee89d55722239ab5ec2429dd020b9125a978))


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
