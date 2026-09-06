-- K4.3: dispatch fairness — per-project concurrency limits.
-- projects.max_running_jobs (NULL = unlimited) bounds how many jobs of one
-- project may hold active leases at once, so a chatty project cannot starve
-- the fleet; the claim query skips candidates whose project is at its cap.

ALTER TABLE projects
    ADD COLUMN IF NOT EXISTS max_running_jobs INTEGER
        CHECK (max_running_jobs IS NULL OR max_running_jobs >= 1);
