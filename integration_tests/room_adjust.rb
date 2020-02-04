require 'json'
require 'jwt'
require 'net/http'
require 'securerandom'
require 'time'
require 'uri'
require 'ulms_client'

# Connect.
me = agent('alpha', account('test', 'dev.usr.example.org'))
event = account('event', 'dev.svc.example.org')

conn = connect host: 'localhost', port: 1883, agent: me, mode: 'service'

# Create room.
started_at = Time.now.to_i
finished_at = started_at + 100

response = conn.make_request 'room.create', to: event, payload: {
  audience: 'dev.usr.example.org',
  time: [started_at, finished_at],
  tags: { webinar_id: '123' }
}

assert response.properties['status'] == '201'
room_id = response.payload['id']

# Bulk add some events with events-api including cut-start/stop.
# We use bulk endpoint to be able to set `created_at` date.
claims = {
  iss: 'iam.dev.usr.example.org',
  sub: me.account.label,
  aud:me.account.audience
}

key = File.read('../docker/backend/conf.d/org.usr.example.iam.key')
token = JWT.encode(claims, key, 'HS256')

def make_backend_request(method, path, token, body = nil)
  uri = URI("http://0.0.0.0:8000/api/v2/#{path}")
  request = method.new(uri)
  request['Content-Type'] = 'application/json'
  request['Authorization'] = "Bearer #{token}"
  request.body = body.to_json if body
  Net::HTTP.start(uri.hostname, uri.port) { |http| http.request(request) }
end

def build_event_metadata(me, created_at, type)
  {
    account_id: me.account.label,
    conn_id: "conn_id_1",
    random_id: SecureRandom.hex(),
    created_at: Time.at(created_at).utc.iso8601,
    type: type,
  }
end

response = make_backend_request(
  Net::HTTP::Post,
  "internal/rooms/#{room_id}/bulk-events",
  token,
  [
    build_event_metadata(me, started_at + 10, 'message'),
    build_event_metadata(me, started_at + 20, 'stream'),
    build_event_metadata(me, started_at + 30, 'message'),
    build_event_metadata(me, started_at + 40, 'stream'),
    build_event_metadata(me, started_at + 50, 'message'),
    build_event_metadata(me, started_at + 60, 'message'),
  ]
)

assert response.is_a?(Net::HTTPNoContent)

# One can not simply put bulk events with data into events-api because this endpoint doesn't
# accept `data` field. So we have to call PATCH endpoint for each event >_<
events_data = [
  { message: 'message 1' },
  { cut: 'start' },
  { message: 'message 2' },
  { cut: 'stop' },
  { message: 'message 3' },
  { message: 'message 4' }
]

response = make_backend_request(
  Net::HTTP::Get,
  "dev.usr.example.org/rooms/#{room_id}/events?direction=forward",
  token
)

assert response.is_a?(Net::HTTPSuccess)
response_body = JSON.parse(response.body)

response_body['events'].zip(events_data).each do |event, data|
  response = make_backend_request(
    Net::HTTP::Patch,
    "dev.usr.example.org/rooms/#{room_id}/events/#{event['type']}/#{event['id']}",
    token,
    data: data
  )

  assert response.is_a?(Net::HTTPSuccess)
end

# Subscribe to notifications topic.
conn.subscribe "apps/event.dev.svc.example.org/api/v1/audiences/dev.usr.example.org/rooms/#{room_id}"

# Adjust the room asynchronously.
response = conn.make_request 'room.adjust', to: event, payload: {
  id: room_id,
  started_at: Time.at(started_at).utc.iso8601,
  segments: [[0, 45_000], [55_000, 70_000]],
  offset: 0,
}

assert response.properties['status'] == '202'

# Wait for the notification.
notification = conn.receive(10) do |msg|
  msg.properties['type'] == 'event' &&
    msg.payload['tags']['webinar_id'] == '123'
end

assert notification.payload['status'] == 'success'
puts notification.payload

original_room_id = notification.payload['original_room_id']
modified_room_id = notification.payload['modified_room_id']

assert notification.payload['modified_segments'] == [[0, 19999], [39999, 60000]]

# Assert events and their timestamps in the original room.
response = make_backend_request(
  Net::HTTP::Get,
  "dev.usr.example.org/rooms/#{original_room_id}/events?direction=forward",
  token
)

assert response.is_a?(Net::HTTPSuccess)
events = JSON.parse(response.body)['events']

assert events[0]['type'] == 'message'
assert events[0]['data'] == { 'message' => 'message 1' }
assert events[0]['offset'] == 9999

assert events[1]['type'] == 'message'
assert events[1]['data'] == { 'message' => 'message 2' }
assert events[1]['offset'] == 29999

assert events[2]['type'] == 'stream'
assert events[2]['data'] == { 'cut' => 'start' }
assert (44000..45000).include?(events[2]['offset'])

assert events[3]['type'] == 'stream'
assert events[3]['data'] == { 'cut' => 'stop' }
assert (44000..45000).include?(events[3]['offset'])

assert events[4]['type'] == 'message'
assert events[4]['data'] == { 'message' => 'message 3' }
assert events[4]['offset'] == 45000

assert events[5]['type'] == 'message'
assert events[5]['data'] == { 'message' => 'message 4' }
assert events[5]['offset'] == 49999

assert events[6]['type'] == 'stream'
assert events[6]['data'] == { 'cut' => 'start' }
assert (59000..60000).include?(events[6]['offset'])

# Assert events and their timestamps in the modified room.
response = make_backend_request(
  Net::HTTP::Get,
  "dev.usr.example.org/rooms/#{modified_room_id}/events?direction=forward",
  token
)

assert response.is_a?(Net::HTTPSuccess)
events = JSON.parse(response.body)['events']

assert events[0]['type'] == 'message'
assert events[0]['data'] == { 'message' => 'message 1' }
assert events[0]['offset'] == 9999

assert events[1]['type'] == 'stream'
assert events[1]['data'] == { 'cut' => 'start' }
assert (19000..20000).include?(events[1]['offset'])

assert events[2]['type'] == 'stream'
assert events[2]['data'] == { 'cut' => 'stop' }
assert (19000..20000).include?(events[2]['offset'])

assert events[3]['type'] == 'message'
assert events[3]['data'] == { 'message' => 'message 2' }
assert events[3]['offset'] == 19999

assert events[4]['type'] == 'stream'
assert events[4]['data'] == { 'cut' => 'start' }
assert (24000..25000).include?(events[4]['offset'])

assert events[5]['type'] == 'stream'
assert events[5]['data'] == { 'cut' => 'stop' }
assert (24000..25000).include?(events[5]['offset'])

assert events[6]['type'] == 'message'
assert events[6]['data'] == { 'message' => 'message 3' }
assert events[6]['offset'] == 25000

assert events[7]['type'] == 'message'
assert events[7]['data'] == { 'message' => 'message 4' }
assert events[7]['offset'] == 29999

assert events[8]['type'] == 'stream'
assert events[8]['data'] == { 'cut' => 'start' }
assert (39000..40000).include?(events[8]['offset'])
