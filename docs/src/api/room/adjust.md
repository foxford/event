# room.adjust

Create derived _original_ and _modified_ [rooms](../room.md#room) with shifted
[events](../event.md#event) according to _segments_ and
[stream editing events](../event.md#stream-editing-events).

The purpose is to keep events in sync with a fragmented video stream and moderator's editions.

For more information on how it works, see the [Room adjustment](../../impl/room_adjustment.md).

This endpoint is intended for calling only a tenant.

## Authorization

The tenant authorizes the current _agent_ for `update` action on `["rooms", room_id]` object.

## Multicast request

Name       | Type         | Default    | Description
---------- | ------------ | ---------- | --------------------------------------------------------
id         | uuid         | _required_ | The real-time room identifier.
started_at | int          | _required_ | The conference room's opening time for error compensation in milliseconds.
segments   | [[int, int]] | _required_ | Start/stop millisecond timestamp pairs relative to video segments's `started_at`
offset     | int          | _required_ | Pre-roll length in milliseconds.

## Unicast response

**Status:** 202.

**Payload:** empty object.

Receiving the response only means that the actual calculation has been started asynchronously.
The actual result comes with a notification.

## Broadcast event

**URI:** `audiences/:audience/events`

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
original_room_id  | uuid         | _required_ | Original room's identifier with applied segments only.
modified_room_id  | uuid         | _required_ | Modified room's identifier with applied stream editing events.
modified_segments | [[int, int]] | _required_ | Segments edited with stream editing events.

`result` object in case of `error` status:

Name  | Type                         | Default    | Description
----- | ---------------------------- | ---------- | ---------------------------------
error | rfc7807 problem details json | _required_ | Error description.
