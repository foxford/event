# ban.list

Read bans in [room](../room.md#room).

## Authorization

The tenant authorizes the current _agent_ for a `update` action on `["classrooms", classroom_id]` object.

## Multicast request

Name                 | Type     | Default    | Description
-------------------- | -------- | ---------- | ---------------------------------------------------------------
room_id              | string   | _required_ | The room's identifier.

## Unicast response

**Status:** 200.

**Payload:** list of room ban objects:

Name         | Type       | Default    | Description
------------ | ---------- | ---------- | ----------------------------------------------------
account_id   | account_id | _required_ | Account id of a banned user
created_at   | int        | _required_ | Ban timestamp
reason       | string     | _optional_ | Ban reason
