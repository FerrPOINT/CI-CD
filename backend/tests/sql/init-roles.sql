CREATE ROLE forge_runtime LOGIN PASSWORD 'forge_test_runtime';
GRANT CONNECT ON DATABASE forge_test_cicd TO forge_runtime;
