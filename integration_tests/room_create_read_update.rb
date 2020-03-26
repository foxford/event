require 'ulms_client'

me = agent('alpha', account('test', 'dev.usr.example.org'))
event = account('event', 'dev.svc.example.org')

conn = connect host: 'localhost', port: 1883, agent: me

# Create room.
now = Time.now.to_i
time = [now + 3600, now + 7200]

response = conn.make_request 'room.create', to: event, payload: {
  audience: 'dev.usr.example.org',
  time: time,
  tags: { webinar_id: '123' }
}

assert response.properties['status'] == '201'
assert response.payload['audience'] = 'dev.usr.example.org'
assert response.payload['time'] == time
assert response.payload['tags']['webinar_id'] == '123'
puts response.payload

# Read room.
room_id = response.payload['id']
response = conn.make_request 'room.read', to: event, payload: { id: room_id }

assert response.properties['status'] == '200'
assert response.payload['audience'] = 'dev.usr.example.org'
assert response.payload['time'] == time
assert response.payload['tags']['webinar_id'] == '123'
puts response.payload

# Update room.
time = [now + 7200, now + 10800]

response = conn.make_request 'room.update', to: event, payload: {
  id: room_id,
  time: time,
  tags: { webinar_id: '456' }
}

assert response.properties['status'] == '200'
assert response.payload['id'] = room_id
assert response.payload['audience'] = 'dev.usr.example.org'
assert response.payload['time'] == time
assert response.payload['tags']['webinar_id'] == '456'
puts response.payload

# Read updated room.
response = conn.make_request 'room.read', to: event, payload: { id: room_id }

assert response.properties['status'] == '200'
assert response.payload['id'] = room_id
assert response.payload['audience'] = 'dev.usr.example.org'
assert response.payload['time'] == time
assert response.payload['tags']['webinar_id'] == '456'
puts response.payload
