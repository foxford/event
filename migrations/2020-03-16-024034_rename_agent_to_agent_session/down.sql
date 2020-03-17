CREATE TYPE agent_status as ENUM ('in_progress', 'ready');

CREATE TABLE agent (
    id UUID DEFAULT gen_random_uuid(),
    agent_id agent_id NOT NULL,
    room_id UUID NOT NULL,
    status agent_status NOT NULL DEFAULT 'in_progress',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    FOREIGN KEY (room_id) REFERENCES room (id) ON DELETE CASCADE,
    UNIQUE (agent_id, room_id),
    PRIMARY KEY (id)
);

INSERT INTO agent (id, agent_id, room_id, status, created_at)
SELECT
    id,
    agent_id,
    room_id,
    (CASE status
        WHEN 'claimed' THEN 'in_progress'
        WHEN 'started' THEN 'ready'
    END)::agent_status AS status,
    created_at
FROM agent_session
WHERE status IN ('claimed', 'started');

DROP TABLE agent_session;
DROP TYPE agent_session_status;
