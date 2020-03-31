# Room entrance

![Room entrance diagram](room_entrance.png)

1. An _agent_ sends a [room.enter](../api/room/enter.md) request.
2. Event service creates an [agent](../api/agent.md#agent) entry in the DB at `in_progress` status
   and sends `subscription.create` request to the broker[^1].
   That occurres at `endpoint::room::EnterHandler`.
3. The broker subscribes the _agent_ to the _room_ _events'_ topic specified at the request and
   sends a `subscription.create` multicast event to the event service.
4. Also, the broker sends 202 response for the request (1) that the entrance process started.
   So for the _agent_, all this scheme is a "black box": it just sends a request and receives
   a response.
5. The event service:
   - handles the multicast event (3),
   - updates _agent_ status from (2) to `ready`,
   - sends a broadcast event to the _room_ _events'_ topic so other _agents_ could know
   that this _agent_ becomes online. This _agent_ also receives this notification (the confirmation that it has entered the room).
   
   The implementation of this step
   is in `endpoint::subscription::CreateHandler`.

[^1]: It's being done through a unicast request to the _agent's_ inbox topic
      (in fact it's like sending a request to the agent). That is to route the request to the
      broker's node which the _agent_ is connected to. Broker nodes don't share their dynamic
      subscriptions state, so only that node knows that it handles the _agent_.
