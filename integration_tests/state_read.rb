require 'ulms_client'

me = agent('alpha', account('test', 'dev.usr.example.org'))
event = account('event', 'dev.svc.example.org')

conn = connect host: 'localhost', port: 1883, agent: me

# Create room.
response = conn.make_request 'room.create', to: event, payload: {
  audience: 'dev.usr.example.org',
  time: [Time.now.to_i, Time.now.to_i + 1000],
  tags: { webinar_id: '123' }
}

assert response.properties['status'] == '201'
room_id = response.payload['id']
room_created_at = response.payload['created_at']

# Enter room.
response = conn.make_request 'room.enter', to: event, payload: { id: room_id }
assert response.properties['status'] == '202'

# Wait to be appear actually in the room to avoid race condition.
conn.receive do |msg|
  msg.properties['type'] == "event" &&
    msg.properties['label'] == 'room.enter' &&
    msg.payload['agent_id'] == me.to_s &&
    msg.payload['id'] == room_id
end

# Create an event.
response = conn.make_request 'event.create', to: event, payload: {
  room_id: room_id,
  type: 'message',
  set: 'messages',
  label: 'message-1',
  data: { message: 'message 1, version 1' }
}

assert response.properties['status'] == '201'

# Create two more event in the same set with different labels.
response = conn.make_request 'event.create', to: event, payload: {
  room_id: room_id,
  type: 'message',
  set: 'messages',
  label: 'message-2',
  data: { message: 'message 2, version 1' }
}

assert response.properties['status'] == '201'

# Create another event with the same set & label.
response = conn.make_request 'event.create', to: event, payload: {
  room_id: room_id,
  type: 'message',
  set: 'messages',
  label: 'message-2',
  data: { message: 'message 2, version 2' }
}

assert response.properties['status'] == '201'

# Create one more event with a different label.
response = conn.make_request 'event.create', to: event, payload: {
  room_id: room_id,
  type: 'message',
  set: 'messages',
  label: 'message-3',
  data: { message: 'message 3, version 1' }
}

assert response.properties['status'] == '201'

# Now we have 3 messages and the second of them has 2 versions.
# Read the first page of the actual state.
response = conn.make_request 'state.read', to: event, payload: {
  room_id: room_id,
  sets: %w(messages),
  limit: 2,
}

assert response.properties['status'] == '200'
assert response.payload['messages'].length == 2
assert response.payload['messages'][0]['label'] == 'message-3'
assert response.payload['messages'][0]['data']['message'] == 'message 3, version 1'
assert response.payload['messages'][1]['label'] == 'message-2'
assert response.payload['messages'][1]['data']['message'] == 'message 2, version 2'
assert response.payload['has_next'] == true

# Read the next page of the state.
response = conn.make_request 'state.read', to: event, payload: {
  room_id: room_id,
  sets: %w(messages),
  occurred_at: response.payload['messages'].last['original_occurred_at'],
  limit: 2,
}

assert response.properties['status'] == '200'
assert response.payload['messages'].length == 1
assert response.payload['messages'][0]['label'] == 'message-1'
assert response.payload['messages'][0]['data']['message'] == 'message 1, version 1'
assert response.payload['has_next'] == false
