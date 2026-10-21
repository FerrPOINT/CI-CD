-- Additive opt-in only. Activation requires the complete serving cohort;
-- no public API enables this partial source implementation.
CREATE TABLE forge_space_admission_state (
    singleton boolean PRIMARY KEY CHECK(singleton),
    active boolean NOT NULL DEFAULT false
);
INSERT INTO forge_space_admission_state(singleton,active) VALUES(true,false);
