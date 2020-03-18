# edition.commit

Create new room from an edition by applying edition changes to source room events.

Performs events shifting as described in [`room.adjust`](../room/adjust.md).

## Authorization

The tenant authorizes the current _agent_ for `update` action on `["rooms", room_id]` object.

## Multicast request

Name  | Type       | Default    | Description
----- | ---------- | ---------- | ------------------------------------------------------------
id    | uuid       | _required_ | Edition id

## Unicast response

**Status:** 202.

**Payload:** empty object.

Received response signals that asynchronous commit task has started. Notification will be sent on the task completion.

## Broadcast event

**URI:** `audiences/:audience/events`

**Label:** `edition.commit`

**Payload:**

Name   | Type   | Default    | Description
------ | ------ | ---------- | -----------------------------------
status | string | _required_ | Task result status: success | error.
tags   | json   | _optional_ | The room's tags.
result | json   | _required_ | Result object (see below).

`result` object in case of `success` status:

Name              | Type         | Default    | Description
----------------- | ------------ | ---------- | ---------------------------------
source_room_id    | uuid         | _required_ | Source room's identifier.
commited_room_id  | uuid         | _required_ | Commited room's identifier with applied stream editing events and changes..
modified_segments | [[int, int]] | _required_ | Segments edited with stream editing events.

`result` object in case of `error` status:

Name  | Type                         | Default    | Description
----- | ---------------------------- | ---------- | ---------------------------------
error | rfc7807 problem details json | _required_ | Error description.
