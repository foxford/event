# Overview

Event service is intended to store and broadcast event data.

The [event](api/event.md#event) is a schemaless JSON document representing processes that took place
in the [room](api/room.md#room).

The event may be a text message, drawing, file upload, UI layout change, etc.
The service does not depend on event data specifics except [stream editing](api/event.md#stream-editing-events) events.

All online [agents](api/agent.md#agent) in the room get notified on these
events as they happen.

One can retrieve events [history](api/event/list.md) or the aggregated room
[state](api/state.md#state) on a certain point of time.

A tenant can [adjust](api/room/adjust.md) the real-time room
by applying segments ([conference][conference] service) and stream editing events (created by a moderator) during the translation once it finished.

[conference]:https://docs.netology-group.services/conference/index.html
