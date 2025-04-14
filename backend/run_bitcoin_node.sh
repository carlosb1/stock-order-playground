docker run --rm -d \
  --name bitcoind-regtest \
  -p 18443:18443 \
  -p 18444:18444 \
  bitcoin/bitcoin \
  -printtoconsole \
  -regtest=1 \
  -rpcallowip=172.17.0.0/16 \
  -rpcbind=0.0.0.0 \
  -fallbackfee=0.0001 \
  -rpcauth='foo:7d9ba5ae63c3d4dc30583ff4fe65a67e$9e3634e81c11659e3de036d0bf88f89cd169c1039e6e09607562d54765c649cc'

docker exec -i bitcoind-regtest sh -c 'cat > /root/.bitcoin/bitcoin.conf' <<EOF
regtest=1
server=1
rpcuser=foo
fallbackfee=0.0001
rpcpassword=qDDZdeQ5vw9XXFeVnXT4PZ--tGN2xNjjR4nrtyszZx0=
EOF
docker exec bitcoind-regtest bitcoin-cli -regtest -rpcuser=foo -rpcpassword=qDDZdeQ5vw9XXFeVnXT4PZ--tGN2xNjjR4nrtyszZx0= createwallet "testwallet"
