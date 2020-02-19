# Overview

Event service is intended for storing and broadcasting event data.

An [event](api/event.md#event) is a schemaless JSON document representing something that took place
in a [room](api/room.md#room).

An event may be a text message, drawing, file upload, UI layout change etc.
The service does not depend on event data specifics [except](api/event.md#stream-editing-events)
stream editing events.

All online [agents](api/agent.md#agent) in the room get notified on these
events as they happen.

One can retrieve events [history](api/event/list.md) or an aggregated room
[state](api/state.md#state) on a certain point of time.

Once a real-time room is finished it can be [adjusted](api/room/adjust.md) by the tenant
by applying segments from [conference][conference] serivce and stream editing events created
by a moderator during the translation.

[conference]:https://docs.netology-group.services/conference/index.html
