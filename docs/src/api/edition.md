# Edition

An _edition_ is a set of changes to be applied to a [room](room.md#room) copy to edit the room.

When edition is considered complete it can be [_commited_](edition/commit.md).

## Properties

Name            | Type     | Default    | Description
----------------| -------- | ---------- | -------------------------------------------------
id              | uuid     | _required_ | The edition's identifier.
source_room_id  | uuid     | _required_ | The source room's identifier.
created_by      | agent_id | _required_ | An agent who created this edition.
created_at      | int      | _required_ | The edition's absolute creation timestamp in seconds.
