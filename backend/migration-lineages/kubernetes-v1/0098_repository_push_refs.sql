ALTER TABLE repository_push_configs ADD COLUMN refs TEXT[] NOT NULL DEFAULT '{}';
