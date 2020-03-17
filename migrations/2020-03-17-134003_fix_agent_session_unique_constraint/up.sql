ALTER TABLE agent_session
DROP CONSTRAINT IF EXISTS agent_session_agent_id_room_id_key;

ALTER TABLE agent_session
ADD CONSTRAINT agent_session_room_id_agent_id_excl
EXCLUDE (room_id with =, agent_id with =)
WHERE (status <> 'finished');
