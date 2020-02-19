# room.adjust

Create derived _original_ and _modified_ [rooms](../room.md#room) with shifted
[events](../event.md#event) according to _segments_ and
[stream editing events](../event.md#stream-editing-events).

The purpose is to keep events in sync with fragmented video stream and moderator edits.

For more information on how it works see [Room adjustment](../../impl/room_adjustment.md).

This endpoint is intended for calling only a tenant.

## Authorization

The current _agent's_ _account_ must be a configured _trusted account_ of the tenant for the
_room's_ _audience_.

## Parameters

Name       | Type         | Default    | Description
---------- | ------------ | ---------- | --------------------------------------------------------
id         | uuid         | _required_ | The real-time room identifier.
started_at | int          | _required_ | The conference room opening time for error compensation in seconds.
segments   | [[int, int]] | _required_ | Start-stop millisecond timestamp pairs relative to started_at of video segments
offset     | int          | _required_ | Pre-roll length in milliseconds.

## Response

**Status:** 202.

**Payload:** empty object.

Receiving the response only means that the actual calculation has been started asynchronously.
The actual result comes with a notification.

## Notification

**URI:** `audiences/:audience/rooms/:room_id`

**Label:** `room.adjust`

**Payload:**

Name   | Type   | Default    | Description
------ | ------ | ---------- | -----------------------------------
status | string | _required_ | Task result status: success | error.
tags   | json   | _optional_ | The room's tags.
result | json   | _required_ | Result object (see below).

`result` object in case of `success` status:

Name              | Type         | Default    | Description
----------------- | ------------ | ---------- | ---------------------------------
original_room_id  | uuid         | _required_ | Identifier of the original room with only segments applied.
modified_room_id  | uuid         | _required_ | Identifier of the modified room with stream editing events also applied.
modified_segments | [[int, int]] | _required_ | Segments edited with stream editing events.

`result` object in case of `error` status:

Name  | Type                         | Default    | Description
----- | ---------------------------- | ---------- | ---------------------------------
error | rfc7807 problem details json | _required_ | Error description.
