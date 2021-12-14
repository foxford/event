# edition.commit

Create new room from an edition by applying edition changes to source room events.

Performs events shifting as described in [`room.adjust`](../room/adjust.md).

## Authorization

The tenant authorizes the current _agent_ for `update` action on `["rooms", room_id]` object.

## Multicast request

Name   | Type       | Default    | Description
------ | ---------- | ---------- | ------------------------------------------------------------
id     | uuid       | _required_ | Edition id
offset | i64        | 0          | Offset to move segments in milliseconds

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


## Request example

    ```json
    {
        "edition_id": "123e4567-e89b-4296-99a5-9f8acc922799"
    }
    ```

## Broadcast event example

    ```bash
    "gateway-type": "event"
    "gateway-label": "edition.commit"
    "gateway-agent-id": "event-0.event.svc.netology-group.services"
    ```

    - Success

        ```json
        {
            "tags": {"webinar_id": "12345"},
            "status": "success",
            "source_room_id": "198b8e6b-80af-4296-99a5-9f8acc922788",
            "committed_room_id": "208b8e6b-80af-4296-99a5-0a1e45283199",
            "modified_segments": [[0, 200], [800, 4000]],
        }
        ```

    - Failure

        ```json
        {
            "tags": {"webinar_id": "12345"},
            "status": "failure",
            "error": ...
        }
