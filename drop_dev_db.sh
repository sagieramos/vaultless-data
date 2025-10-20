# 1. Execute psql inside the docker container
#    -t: no column headers
#    -c: run the command and exit
#    -U: specifies the user (vaultless)
#    -d: specifies the database to connect to (connecting to 'postgres' is safer when dropping the other database)
#
# NOTE: The command must connect to a *different* database (like 'postgres') 
#       to successfully drop the target database ('vaultless_db').

docker exec -it vaultless-postgres psql \
  -U vaultless \
  -d postgres \
  -c "DROP DATABASE vaultless_db;"
