# agent.update

Bans or unbans provided [account](../agent.md#agent) in a [room](../room.md#room) from creating messages.

## Authorization

The tenant authorizes the current _agent_ for `create` action on `["rooms", room_id, "claims", "role", "authors", current_account_id]`.

## Multicast request

Name             | Type                 | Default    | Description
---------------- | -------------------- | ---------- | ------------------
room_id          | string               | _required_ | The room's identifier.
agent_id         | agent_id             | _required_ | The agent id of account to ban.
value            | bool                 | _required_ | Whether to ban (value = true) or unban (value = false) the account.

## Unicast response

**Status:** 200.

**Payload:** empty json object


## Broadcast event

A notification is being sent to all [agents](../agent.md#agent) that
[are in](../room/enter.md) the room.

**URI:** `rooms/:room_id/events`

**Label:** `agent.update`.

**Payload:**

Name             | Type        | Default    | Description
---------------- | ----------- | ---------- | -----------------------------------
account_id       | account_id  | _required_ | Altered account.
banned           | bool        | _required_ | Whether the account was banned or unbanned.
