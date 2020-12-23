# room.create

Create a [room](../room.md#room).

## Authorization

The tenant authorizes the current _agent_ for `create` action on `["rooms"]` object.

## Multicast request

Name             | Type       | Default    | Description
---------------- | ---------- | ---------- | --------------------------------------------------------------
audience         | string     | _required_ | The room audience.
time             | [int, int] | _required_ | A [lt, rt) range of unix time (seconds).
tags             | json       | _optional_ | Tenant-specific JSON object associated with the room.
preserve_history | bool       | true       | Disables automatic cleanup of non-state events for each label.

## Unicast response

**Status:** 201.

**Payload:** [room](../room.md#room) object.

## Broadcast event

A notification is being sent to the _audience_ topic.

**URI:** `audiences/:audience/events`

**Label:** `room.create`.

**Payload:** [room](../room.md#room) object.
