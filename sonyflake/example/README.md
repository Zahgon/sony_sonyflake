# Example

This example runs Sonyflake on AWS Elastic Beanstalk.

## Setup

1. Install the linux/amd64 target if building from another platform.

   ```bash
   rustup target add x86_64-unknown-linux-gnu
   ```

2. Build sonyflake_server in the example directory.

   ```bash
   ./linux64_build.sh
   ```

3. Upload the example directory to AWS Elastic Beanstalk.
