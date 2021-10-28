# HTTP

__NOTE__: if an id is present in mqtt payload and in a corresponding http route - http payload should omit this id.

List of currently present http routes:

Path                        | Method    | Description
------------                | -------   | ------------------------------------------------------------
/rooms                      | POST      | [Create](./room/create.md) room
/rooms/:id                  | GET       | [Read](./room/read.md) room
/rooms/:id                  | PATCH     | [Update](./room/update.md) room
/rooms/:id/adjust           | POST      | [Adjust](./room/adjust.md) room
/rooms/:id/enter            | POST      | [Enter](./room/enter.md) room
/rooms/:id/leave            | POST      | [Leave](./room/leave.md) room
/rooms/:id/dump_events      | POST      | [Dump](./room/dump_events.md) room events
/rooms/:id/locked_types     | POST      | [Update](./room/locked_types.md) locked types in room
/rooms/:id/events           | GET       | [List](./event/list.md) events
/rooms/:id/events           | POST      | [Create](./event/create.md) event
/rooms/:id/agents           | GET       | [List](./agent/list.md) agents
/rooms/:id/agents           | PATCH     | [Update](./agent/update.md) agent
/rooms/:id/state            | GET       | [Read](./state/read.md) room state
/rooms/:id/editions         | GET       | [List](./edition/list.md) room editions
/rooms/:id/editions         | POST      | [Create](./edition/create.md) edition
/editions/:id               | DELETE    | [Delete](./edition/delete.md) edition
/editions/:id/commit        | POST      | [Commit](./edition/commit.md) edition
/editions/:id/changes       | GET       | [List](./change/list.md) edition changes
/editions/:id/changes       | POST      | [Create](./change/create.md) change
/changes/:id                | DELETE    | [Delete](./change/delete.md) change
