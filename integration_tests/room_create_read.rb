require 'ulms_client'

me = agent('alpha', account('test', 'dev.usr.example.org'))
event = account('event', 'dev.svc.example.org')

conn = connect host: 'localhost', port: 1883, agent: me, mode: 'service'

response = conn.make_request 'room.create', to: event, payload: {
  audience: 'dev.usr.example.org',
  time: [1580173002, 1580174002]
}

assert response.properties['status'] == '201'
assert response.payload['audience'] = 'dev.usr.example.org'
assert response.payload['time'] == [1580173002, 1580174002]
puts response.payload

response = conn.make_request 'room.read', to: event, payload: {
  id: response.payload['id']
}

assert response.properties['status'] == '200'
assert response.payload['audience'] = 'dev.usr.example.org'
assert response.payload['time'] == [1580173002, 1580174002]
puts response.payload
