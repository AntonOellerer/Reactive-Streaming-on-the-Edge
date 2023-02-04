docker tag implementation-monitor 127.0.0.1:5000/monitor
docker tag implementation-sensor 127.0.0.1:5000/sensor
docker tag implementation-cloud_server 127.0.0.1:5000/cloud_server

docker push 127.0.0.1:5000/monitor
docker push 127.0.0.1:5000/sensor
docker push 127.0.0.1:5000/cloud_server

