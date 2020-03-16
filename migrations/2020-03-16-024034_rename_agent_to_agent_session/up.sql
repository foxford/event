CREATE TYPE agent_session_status AS ENUM ('claimed', 'started', 'finished');

CREATE TABLE agent_session (
    id UUID DEFAULT gen_random_uuid(),
    agent_id agent_id NOT NULL,
    room_id UUID NOT NULL,
    status agent_session_status NOT NULL DEFAULT 'claimed',
    time TSTZRANGE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    FOREIGN KEY (room_id) REFERENCES room (id) ON DELETE CASCADE,
    UNIQUE (agent_id, room_id),
    PRIMARY KEY (id)
);

ALTER TABLE agent_session ADD CONSTRAINT agent_session_status_time_check CHECK (
    CASE status
        WHEN 'claimed' THEN
            time IS NULL
        WHEN 'started' THEN
            time IS NOT NULL
            AND LOWER(time) IS NOT NULL
            AND UPPER(time) IS NULL
        WHEN 'finished' THEN
            time IS NOT NULL
            AND LOWER(time) IS NOT NULL
            AND UPPER(time) IS NOT NULL
    END
);

INSERT INTO agent_session (id, agent_id, room_id, status, time, created_at)
SELECT
    id,
    agent_id,
    room_id,
    (CASE status
        WHEN 'in_progress' THEN 'claimed'
        WHEN 'ready' THEN 'started'
    END)::agent_session_status AS status,
    CASE status
        WHEN 'in_progress' THEN NULL
        WHEN 'ready' THEN TSTZRANGE(created_at, NULL, '[)')
    END AS time,
    created_at
FROM agent;

CREATE UNIQUE INDEX agent_session_room_id_agent_id
ON agent_session (room_id, agent_id)
WHERE status != 'finished';

DROP TABLE agent;
DROP TYPE agent_status;
