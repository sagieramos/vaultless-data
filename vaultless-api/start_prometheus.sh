docker run -d -p 9090:9090 \
  --network=host \
  -v $(pwd)/prometheus.yml:/etc/prometheus/prometheus.yml \
  --name prometheus \
  prom/prometheus
