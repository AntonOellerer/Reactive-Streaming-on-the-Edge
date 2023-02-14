docker tag implementation-monitor $REGISTRY_IP:5000/monitor
docker tag implementation-sensor $REGISTRY_IP:5000/sensor
docker tag implementation-cloud_server $REGISTRY_IP:5000/cloud_server

docker push $REGISTRY_IP:5000/monitor
docker push $REGISTRY_IP:5000/sensor
docker push $REGISTRY_IP:5000/cloud_server

