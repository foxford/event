require 'json'
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

# Enter room.
response = conn.make_request 'room.enter', to: event, payload: { id: room_id }
assert response.properties['status'] == '200'

# Create event.
response = conn.make_request 'event.create', to: event, payload: {
  room_id: room_id,
  type: 'message',
  data: { message: 'hello' }
}

assert response.properties['status'] == '201'
assert response.payload['data'] == { 'message' => 'hello' }
event_id = response.payload['id']

# Receive new event notification.
conn.receive do |msg|
  msg.properties['type'] == "event" &&
    msg.properties['label'] == 'event.create' &&
    msg.payload['id'] == event_id
end

# List events.
response = conn.make_request 'event.list', to: event, payload: {
  room_id: room_id,
  type: 'message',
}

assert response.properties['status'] == '200'
assert response.payload.length == 1
assert response.payload[0]['id'] == event_id
assert response.payload[0]['data']['message'] == 'hello'
