# room.create

Create a [room](../room.md#room).

## Authorization

The tenant authorizes the current _agent_ for `create` action on `["rooms"]` object.

## Parameters

Name     | Type       | Default    | Description
-------- | ---------- | ---------- | ------------------------------------------------------------
audience | string     | _required_ | The room audience.
time     | [int, int] | _required_ | A [lt, rt) range of unix time (seconds) or null (unbounded).
tags     | json       | _optional_ | Tenant-specific JSON object associated with the room.

## Response

**Status:** 201.

**Payload:** [room](../room.md#room) object.

## Notification

A notification is being sent to the _audience_ topic.

**URI:** `audiences/:audience/events`

**Label:** `room.create`.

**Payload:** [room](../room.md#room) object.
