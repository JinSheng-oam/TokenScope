CREATE TABLE IF NOT EXISTS project_names (
  device_id TEXT NOT NULL REFERENCES devices(device_id) ON DELETE RESTRICT,
  project_id TEXT NOT NULL CHECK(length(project_id)=68 AND substr(project_id,1,4)='prj_'),
  name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 512),
  updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms >= 0),
  PRIMARY KEY(device_id,project_id)
);
