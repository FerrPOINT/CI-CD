-- Never infer old automation ownership from a mutable name or first admin.
ALTER TABLE schedules ADD COLUMN owner_subject uuid
    CHECK(owner_subject <> '00000000-0000-0000-0000-000000000000');
-- A materialized fire keeps its original owner across edits/retries.
ALTER TABLE schedule_fires ADD COLUMN owner_subject uuid
    CHECK(owner_subject <> '00000000-0000-0000-0000-000000000000');
