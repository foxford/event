require 'ulms_client'

@me = agent('alpha', account('test', 'dev.usr.example.org'))
@event = account('event', 'dev.svc.example.org')

@conn = connect host: 'localhost', port: 1883, agent: @me

def main
  room_id = create_room
  @conn.subscribe "apps/event.dev.svc.example.org/api/v1/audiences/dev.usr.example.org/events"

  event_ids = create_events(room_id)
  edition_id = create_edition(room_id)
  create_changes(edition_id, event_ids)
  new_room_id = commit_edition(edition_id, room_id)

  response = @conn.make_request 'event.list', to: @event, payload: {
    room_id: new_room_id,
    type: 'message',
  }

  assert response.properties['status'] == '200'

  assert response.payload.map{|v| v['type']}.uniq == ['message']
  assert response.payload.map{|v| v['data']['message']} == ['theme', 'hello', 'great', 'tech issues', 'goodbye']

  assert response.payload.length == 5

end

def create_room
  response = @conn.make_request 'room.create', to: @event, payload: {
    audience: 'dev.usr.example.org',
    time: [Time.now.to_i, Time.now.to_i + 100000],
    tags: { webinar_id: '123' }
  }

  assert response.properties['status'] == '201'
  assert response.payload['audience'] = 'dev.usr.example.org'
  assert response.payload['tags']['webinar_id'] == '123'

  response.payload['id']
end

def create_events(room_id)
  response = @conn.make_request 'room.enter', to: @event, payload: { id: room_id }
  assert response.properties['status'] == '200'

  [
    ['message', { message: 'hi' }],
    ['message', { message: 'swearing' }],
    ['message', { message: 'great' }],
    ['stream', { cut: 'start' }],
    ['message', { message: 'tech issues' }],
    ['stream', { cut: 'stop' }],
    ['message', { message: 'bye' }]
  ].map do |t, data|
    response = create_event(room_id, t, data)
    response.payload['id']
  end
end


def create_event(room_id, type, data)
  resp = @conn.make_request 'event.create', to: @event, payload: {
    room_id: room_id,
    type: type,
    data: data
  }

  assert resp.properties['status'] == '201'
  resp
end

def create_edition(room_id)
  response = @conn.make_request 'edition.create', to: @event, payload: {
    room_id: room_id
  }

  assert response.payload['source_room_id'] == room_id

  edition_id = response.payload['id']
end

def create_changes(edition_id, event_ids)
  response = @conn.make_request 'change.create', to: @event, payload: {
    edition_id: edition_id,
    type: 'modification',
    event: {
      event_id: event_ids[0],
      data: {message: "hello"},
    }
  }
  assert response.properties['status'] == '201'

  response = @conn.make_request 'change.create', to: @event, payload: {
    edition_id: edition_id,
    type: 'removal',
    event: {
      event_id: event_ids[1],
    }
  }
  assert response.properties['status'] == '201'

  response = @conn.make_request 'change.create', to: @event, payload: {
    edition_id: edition_id,
    type: 'modification',
    event: {
      event_id: event_ids.last,
      data: {message: "goodbye"},
    }
  }
  assert response.properties['status'] == '201'

  response = @conn.make_request 'change.create', to: @event, payload: {
    edition_id: edition_id,
    type: 'addition',
    event: {
      type: 'message',
      data: {message: "theme"},
      occurred_at: 0,
      created_by: @me
    }
  }
  assert response.properties['status'] == '201'
end

def commit_edition(id, source_room_id)
  response = @conn.make_request 'edition.commit', to: @event, payload: { id: id }
  assert response.properties['status'] == '202'

  msg = @conn.receive do |msg|
    msg.properties['type'] == "event" &&
      msg.properties['label'] == 'edition.commit' &&
      msg.payload['source_room_id'] == source_room_id
  end

  msg.payload['committed_room_id']
end

main()
