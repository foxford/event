# Errors

In case of an error, the response payload is an RFC7807 Problem Details object:

Name   | Type   | Default    | Description
------ | ------ | ---------- | ---------------------------------
type   | string | _required_ | Error type.
title  | string | _required_ | Human-readable short description.
detail | string | _optional_ | Detailed error description.
status | int    | _required_ | HTTP-compatible status code. The same code is in response properties.

## Troubleshooting by status code

- **400 Bad Request** – Failed to parse JSON payload of the message or endpoint-specific validation failed.
- **403 Forbidden** – Authorization failed. Check out Authorization section of the endpoint.
- **404 Not Found** – The entity doesn't exist in the DB or expired.
- **405 Method Not Allowed** – Unknown `method` property value in the request.
- **422 Unprocessable Entity** – DB query error or some logic error.

## Error types

One must rely on the `type` field of the error for error identification, not the `title` nor `status`.
The following types are a part of the service's API and are guaranteed to maintain compatibility.

- `access_denied` – The action was forbidden by [authorization](authz.md#Authorization).
- `agent_not_entered_the_room` – The agent must preliminary make [room.enter](room/enter.md#room.enter) request.
- `authorization_failed` – Authorization request failed due to a network error or another reason.
- `broker_request_failed` – Failed to make a request to the broker.
- `change_not_found` – A [change](change.md#Change) is missing.
- `database_connection_acquisition_failed` – The service couldn't obtain a DB connection from the pool.
- `database_query_failed` – The database returned an error while executing a query.
- `edition_commit_task_failed` – An error in the asynchronous edition commit task called by [edition.commit](edition/commit.md#edition.commit).
- `edition_not_found` – An [edition](edition.md#Edition) is missing.
- `invalid_payload` – Failed to parse the payload because it's schema doesn't match the method's parameters spec.
- `invalid_room_time` – [Room](room.md#room) opening period is wrong. Most likely closing date <= opening date or some of them are nulls.
- `invalid_state_sets` – Zero or too many (> 100) sets passed to [state.read](state/read.md#state.read).
- `invalid_subscription_object` – An object for dynamic subscription is not of format `["rooms", UUID, "events"]`.
- `message_handling_failed` – An incoming message is likely to have non-valid JSON payload or missing required properties.
- `serialization_failed` – JSON serialization failed.
- `stats_collection_failed` – Couldn't collect metrics from one of the sources.
- `publish_failed` – Failed to publish an MQTT message.
- `room_adjust_task_failed` – An error in the asynchronous room adjustment task called by [room.adjust](room/adjust.md#room.adjust).
- `room_not_found` – The [room](room.md#Room) is missing.
- `room_closed` - The [room](room.md#Room) exists but already closed.
- `transient_event_creation_failed` – An error [creating](event/create.md#event.create) a non-persistent event.
- `unknown_method` – An unsupported value in `method` property of the request message.
