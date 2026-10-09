#!/usr/local/bin/python3
"""Exact native Compose transport to the task-owned host SDK bridge; no retries."""
import json
from pathlib import Path
import socket
import sys

MAX_DOCUMENT = 1024 * 1024
CONFIG = Path('/etc/forge-native-qa-transport.json')


def main():
    # Production native commands deliberately clear env; this mount is packet-sealed.
    if CONFIG.is_symlink() or CONFIG.stat().st_size > 4096:
        raise ValueError('Invalid native QA transport config')
    config = json.loads(CONFIG.read_text(encoding='utf-8'))
    if (set(config) != {'version', 'socket'} or type(config['version']) is not int
            or config['version'] != 1 or not isinstance(config['socket'], str)):
        raise ValueError('Invalid native QA transport config')
    path = Path(config['socket'])
    if not path.is_absolute() or '..' in path.parts or str(path) != config['socket']:
        raise ValueError('Absolute native QA bridge socket required')
    request = (json.dumps({'argv': sys.argv[1:]}) + '\n').encode()
    if len(request) > MAX_DOCUMENT:
        raise ValueError('Oversized native QA request')
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(90)
        connection.connect(str(path))
        connection.sendall(request)
        with connection.makefile('rb') as stream:
            response = stream.readline(MAX_DOCUMENT + 1)
    if len(response) > MAX_DOCUMENT or not response.endswith(b'\n'):
        raise ValueError('Invalid native QA response framing')
    value = json.loads(response)
    if (set(value) != {'exit', 'stdout', 'error_class'} or type(value['exit']) is not int
            or value['exit'] not in (0, 1) or not isinstance(value['stdout'], str)
            or value['error_class'] not in (None, 'ValueError', 'RuntimeError', 'OSError',
                'TimeoutError', 'FileNotFoundError', 'PermissionError', 'AssertionError',
                'JSONDecodeError', 'KeyError', 'TypeError', 'TimeoutExpired')):
        raise ValueError('Invalid native QA response')
    if value['exit']:
        sys.stderr.write('NATIVE_QA_COMPOSE_BRIDGE_FAILED:' + str(value['error_class']) + '\n')
    else:
        sys.stdout.write(value['stdout'])
    return value['exit']


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError) as error:
        sys.stderr.write('NATIVE_QA_COMPOSE_BRIDGE_FAILED:' + type(error).__name__ + '\n')
        sys.exit(1)
