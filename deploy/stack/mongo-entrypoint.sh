#!/bin/sh
set -eu
cp /run/bootstrap/mongo-key /tmp/mongo-key
chmod 400 /tmp/mongo-key
chown mongodb:mongodb /tmp/mongo-key
exec /usr/local/bin/docker-entrypoint.sh mongod --replSet rs0 --bind_ip_all --keyFile /tmp/mongo-key
