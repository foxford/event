# room.dump_events

Upload room events to S3 storage to object `s3://eventsdump.{room.audience}/{room.id}.json`.
Uploaded json format would be `{room: Room, events: [Event]}`.

## Authorization

Dispatcher is trusted to perform this action.

## Multicast request

Name  | Type | Default    | Description
----- | ---- | ---------- | --------------------
id    | uuid | _required_ | The room identifier.

## Unicast response

**Status:** 202.

**Payload:** empty object.

Receiving the response only means that the actual task is running asynchronously.
The actual result comes with a notification.
If status is 501 then no task was spawned since there is no S3 client configured.

## Broadcast event

**URI:** `audiences/:audience/events`

**Label:** `room.dump_events`

**Payload:**

Name   | Type   | Default    | Description
------ | ------ | ---------- | -----------------------------------
status | string | _required_ | Task result status: success | error.
tags   | json   | _optional_ | The room's tags.
result | json   | _required_ | Result object (see below).

`result` object in case of `success` status:

Name     | Type         | Default    | Description
-------- | ------------ | ---------- | ---------------------------------
room_id  | uuid         | _required_ | Room id
s3_uri   | string       | _required_ | S3 uri of the object events were dumped to

`result` object in case of `error` status:

Name  | Type                         | Default    | Description
----- | ---------------------------- | ---------- | ---------------------------------
error | rfc7807 problem details json | _required_ | Error description.
