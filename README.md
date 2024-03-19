## DAS-API Testing Suite

This repository is dedicated to the DAS-API Testing Suite, designed to facilitate the comparison of responses from two distinct providers, namely the reference and the testing entities.

Also it has performance tests.

## Utilization Guidelines

1. Begin by installing the **Rust programming language** from its [official website](https://www.rust-lang.org/). It is advisable to use version **1.75.0** for optimal compatibility.
2. Prepare your testing environment by configuring the test parameters as per the instructions provided further down.
3. Populate the test file with the public keys necessary for each method being tested, following the guidance provided below.
4. Configure the logging verbosity level via the **RUST_LOG** environment variable. `info` is the recommended setting for a balanced output.
```bash
export RUST_LOG=info
```
5. Initiate the testing process with the command below, ensuring the path to your configuration file is correctly specified.
```bash
cargo run -- --config-path=/path/to/your/config.json ----test-type=integrity/performance
```

## Configuration Setup

Within the `config/config_example.json` file located in this repository, you will find a template for setting up your configuration. The structure is as follows:
```
{
  "reference_host": "https://example-reference.com",
  "testing_host": "https://example-testing.com",
  "testing_file_path": "/path/to/your/test/file.txt",
  "test_retries": 3,
  "log_differences": false,
  "difference_filter_regexes": [""],
  "num_of_virtual_users": 5,
  "test_duration_time": 10
}
```
* The `reference_host` and `testing_host` parameters denote the URLs of the DAS-API providers under comparison.
* The `testing_file_path` parameter specifies the local file path containing the test public keys.
* The `test_retries` parameter determines the number of attempts for each test before it is deemed unsuccessful. Configurations with values less than 1 will result in an error, whereas a value of 1 signifies immediate failure upon the first unsuccessful attempt.
* The `log_differences` boolean flag controls the logging of discrepancies in failed tests, with a true value enabling this feature.
* The `difference_filter_regexes` provides an array of regular expressions designed to exclude certain disparities from the comparative analysis of provider responses. This feature is particularly useful for ignoring known, inconsequential differences.
* The `num_of_virtual_users` parameter specifies the number of threads that will send requests in parallel mode to the API. **For performance test only**
* The `test_duration_time` parameter specifies the duration, in seconds, for which the test will run. **For performance test only**

For performance tests `testing_host` API will be used.

## Testing keys file

An exemplar file for test keys, `testing_keys/testing_keys_example.txt`, is available within this repository. The format is outlined as follows:
```
Method1:
key1,key2,key3

Method2:
keyA,keyB
```
The permissible methods include `getAsset`, `getAssetProof`, `getAssetsByOwner`, `getAssetsByAuthority`, `getAssetsByGroup`, and `getAssetsByCreator`. The testing suite will encompass all listed methods, with keys for each method being delineated by commas, allowing for multiline entries and trailing commas.