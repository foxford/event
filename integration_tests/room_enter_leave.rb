require 'ulms_client'

agent = agent('alpha', account('test', 'dev.usr.example.org'))
event = account('event', 'dev.svc.example.org')

conn = connect host: 'localhost', port: 1883, agent: agent
conn.subscribe "agents/#{agent}/api/v1/in/#{event}"

# Create a room.
response = conn.make_request 'room.create', to: event, payload: {
  audience: 'dev.usr.example.org',
  time: [Time.now.to_i, Time.now.to_i + 10000],
  tags: { webinar_id: '123' }
}

assert response.properties['status'] == '201'
room_id = response.payload['id']

# Enter the room.
response = conn.make_request 'room.enter', to: event, payload: { id: room_id }
assert response.properties['status'] == '200'

# List active agents in the room.
response = conn.make_request 'agent.list', to: event, payload: { room_id: room_id }
assert response.properties['status'] == '200'
assert response.payload[0]['agent_id'] == agent.to_s

# Leave the room.
response = conn.make_request 'room.leave', to: event, payload: { id: room_id }
assert response.properties['status'] == '200'

# List active agents in the room.
response = conn.make_request 'agent.list', to: event, payload: { room_id: room_id }
assert response.properties['status'] == '200'
assert response.payload.empty?
