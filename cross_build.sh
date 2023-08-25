CROSS_CONTAINER_ENGINE=podman

cd motor_driver
cross build --target aarch64-unknown-linux-gnu

cd ../motor_monitor_oo
cross build --target aarch64-unknown-linux-gnu

cd ../motor_monitor_rx
cross build --target aarch64-unknown-linux-gnu

cd ..