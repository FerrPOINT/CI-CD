#!/usr/bin/env python3
"""Real PG17 frozen row-expression proof; not protocol/lease/receipt authority."""
import ast
import importlib.util
import json
from pathlib import Path
import subprocess
import sys

project, manifest, executor = sys.argv[1:]
pg = ['docker', '--context', 'rootless', 'compose', '-p', project, '-f', manifest, 'exec', '-T', 'postgres']


def sql(database, statement):
    result = subprocess.run(pg + ['psql', '-X', '-qAt', '-v', 'ON_ERROR_STOP=1', '-U', 'postgres',
                                 '-d', database, '-c', statement], capture_output=True, text=True, timeout=30)
    assert result.returncode == 0, result.stderr
    return result.stdout.strip()


spec = importlib.util.spec_from_file_location('frozen_owner', executor)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
tree = ast.parse(Path(executor).read_text())
method = next(node for node in ast.walk(tree) if isinstance(node, ast.FunctionDef) and node.name == 'fingerprint')
assignments = [node for node in ast.walk(method) if isinstance(node, ast.Assign)
               and any(isinstance(target, ast.Name) and target.id == 'rows' for target in node.targets)]
assert len(assignments) == 1
call = assignments[0].value
assert isinstance(call, ast.Call) and isinstance(call.func, ast.Attribute) and call.func.attr == 'sql'
expression = compile(ast.Expression(call.args[1]), '<frozen-production-row-query>', 'eval')
settings = "SET timezone='UTC'; SET extra_float_digits=3; SET datestyle='ISO,YMD'; SET intervalstyle='postgres'; SET bytea_output='hex'; SET lc_monetary='C';"
database = 'qa_record_text'
assert int(sql('postgres', 'SHOW server_version_num;')) // 10000 == 17
sql('postgres', f'CREATE DATABASE {database};')


def document(table, prefix=''):
    query = eval(expression, {'__builtins__': {}}, {'identifier': 'public."' + table + '"'})
    return sql(database, prefix + query)


def fingerprint(table, prefix=''):
    encoded = document(table, prefix)
    records = json.loads(encoded)
    assert type(records) is list and all(type(row) is str for row in records)
    return module.row_fingerprint(encoded)


try:
    sql(database, 'CREATE TABLE public.qa_precision(n numeric,j json,z text);')
    sql(database, "INSERT INTO public.qa_precision VALUES (1.0000000000000000000000000001,'{\"k\":1,\"k\":2}',NULL);")
    first = fingerprint('qa_precision')
    sql(database, 'UPDATE public.qa_precision SET n=1.0000000000000000000000000002;')
    assert fingerprint('qa_precision') != first, 'distinct numeric values collide'
    print('PG17_RECORD_NUMERIC_PRECISION:PASS', flush=True)
    for raw in ('{"k":1,"k":2}', '{"k":2}', '{ "k" : 2 }', '{"k":2.0}'):
        sql(database, "UPDATE public.qa_precision SET j=" + "'" + raw + "'::json;")
        current = fingerprint('qa_precision')
        if raw != '{"k":1,"k":2}':
            assert current != previous, 'raw json lexemes or duplicate keys collide'
        previous = current
    print('PG17_RECORD_RAW_JSON_LEXEMES_AND_DUPLICATE_KEYS:PASS', flush=True)
    null = fingerprint('qa_precision')
    sql(database, "UPDATE public.qa_precision SET z='';")
    assert fingerprint('qa_precision') != null, 'NULL and empty string collide'
    print('PG17_RECORD_NULL_VS_EMPTY:PASS', flush=True)
    one = fingerprint('qa_precision')
    sql(database, 'INSERT INTO public.qa_precision SELECT * FROM public.qa_precision;')
    assert fingerprint('qa_precision') != one, 'duplicate rows lost'
    sql(database, 'CREATE TABLE public.qa_order (LIKE public.qa_precision); '
        "INSERT INTO public.qa_precision VALUES (9,'null','x'); "
        'INSERT INTO public.qa_order SELECT * FROM public.qa_precision ORDER BY n DESC;')
    assert fingerprint('qa_precision') == fingerprint('qa_order'), 'physical row order changes proof'
    print('PG17_RECORD_DUPLICATES_AND_ORDER_INDEPENDENCE:PASS', flush=True)
    sql(database, 'CREATE TABLE public.qa_alias(t text,n numeric,j json); '
        "INSERT INTO public.qa_alias VALUES ('same',1.0000000000000000000000000001,'{\"k\":1,\"k\":2}');")
    alias = fingerprint('qa_alias')
    sql(database, 'UPDATE public.qa_alias SET n=1.0000000000000000000000000002;')
    assert fingerprint('qa_alias') != alias, 'column t shadows whole-row alias; non-t changes collide'
    print('PG17_RECORD_ALIAS_COLUMN_T:PASS', flush=True)
    sql(database, 'CREATE TABLE public.qa_typed(f double precision,d date,ts timestamptz,i interval,b bytea,m money); '
        + settings + "INSERT INTO public.qa_typed VALUES (0.10000000000000002,'2025-03-04',"
        "'2025-03-04 05:06:07.123456+05','1 year 2 months 3 days 04:05:06.123456',decode('005cff','hex'),'1234.56');")
    typed = fingerprint('qa_typed')
    hostile = "SET timezone='Pacific/Honolulu'; SET extra_float_digits=-15; SET datestyle='SQL,DMY'; SET intervalstyle='sql_standard'; SET bytea_output='escape';"
    assert fingerprint('qa_typed', hostile) == typed, 'session output defaults leak into row proof'
    sql(database, 'UPDATE public.qa_typed SET f=0.10000000000000003;')
    assert fingerprint('qa_typed') != typed, 'distinct float values collide'
    print('PG17_RECORD_TYPED_OUTPUT_SETTINGS:PASS', flush=True)
    tables = ('qa_precision', 'qa_order', 'qa_alias', 'qa_typed')
    before = {table: fingerprint(table) for table in tables}
    dump = subprocess.run(pg + ['pg_dump', '-U', 'postgres', '-d', database, '--format=custom'], capture_output=True, timeout=30)
    assert dump.returncode == 0, dump.stderr
    restored = database + '_restore'
    sql('postgres', f'CREATE DATABASE {restored};')
    result = subprocess.run(pg + ['pg_restore', '-U', 'postgres', '-d', restored, '--no-owner', '--no-privileges',
                                 '--exit-on-error'], input=dump.stdout, capture_output=True, timeout=30)
    assert result.returncode == 0, result.stderr
    database = restored
    assert {table: fingerprint(table) for table in tables} == before, 'typed row proof changes across actual dump/restore'
    print('PG17_RECORD_ACTUAL_DUMP_RESTORE_PARITY:PASS', flush=True)
    print('ACTUAL_PG17_RECORD_TEXT_SMOKE:PASS', flush=True)
finally:
    sql('postgres', 'DROP DATABASE IF EXISTS qa_record_text_restore WITH (FORCE);')
    sql('postgres', 'DROP DATABASE IF EXISTS qa_record_text WITH (FORCE);')
