<br/>
<p align="center">
  <h3 align="center">S3 Bucket Permanent Permanent Link</h3>

  <p align="center">
    <a href="https://github.com/Timmi6790/s3-bucket-perma-link/issues">Report Bug</a>
    .
    <a href="https://github.com/Timmi6790/s3-bucket-perma-link/issues">Request Feature</a>
  </p>
</p>

<div align="center">

![Docker Image Version (latest semver)](https://img.shields.io/docker/v/timmi6790/s3-bucket-perma-link)
![GitHub Workflow Status](https://img.shields.io/github/actions/workflow/status/Timmi6790/s3-bucket-perma-link/build.yml)
![Issues](https://img.shields.io/github/issues/Timmi6790/s3-bucket-perma-link)
[![codecov](https://codecov.io/gh/Timmi6790/s3-bucket-perma-link/branch/master/graph/badge.svg?token=dDUZjsYmh2)](https://codecov.io/gh/Timmi6790/s3-bucket-perma-link)
![License](https://img.shields.io/github/license/Timmi6790/s3-bucket-perma-link)
[![wakatime](https://wakatime.com/badge/github/Timmi6790/s3-bucket-perma-link.svg)](https://wakatime.com/badge/github/Timmi6790/s3-bucket-perma-link)

</div>

## About The Project

A simple web server to allow pre-defined urls to always access specific S3 bucket resources.

### Installation - Helm chart

- [Helm chart](https://github.com/Timmi6790/helm-charts/tree/main/charts/s3-bucket-perma-link)


### Environment variables

| Environment    	    | Required 	  | Description                         	                                             | Example                                       |
|---------------------|-------------|-----------------------------------------------------------------------------------|-----------------------------------------------|
| S3.ACCESS_KEY  	    | X	          | S3 Access key                        	                                            | e25a2fd93e1049a4bb48d00907d6f4bf.access       |
| S3.SECRET_KEY    	  | X         	 | S3 secret key                     	                                               | a5990007b7a54f83b52594a86c4d520e              |
| S3.HOST    	        | X	          | S3 host url                            	                                          | s3.amazon.com                                 |
| S3.REGION   	       | X	          | S3 region                          	                                              | eu-central-1                                  |
| BUCKET.ENTRIES      | X           | Key to bucket file                                                                | key:bucket,file.txt; key2:bucket2,config.yaml |
| SERVER.HOST 	       | 	           | Server host [Default: 0.0.0.0]	                                                   | 0.0.0.0                                       |
| SERVER.PORT       	 | 	           | Server port [Default: 8080]                           	                           | 9090                                          |
| SENTRY_DSN     	    | 	           | Sentry DSN                          	                                             |                                               |
| LOG_LEVEL  	        | 	           | Log level [FATAL, ERROR, WARN, INFO, DEBUG, TRACE, ALL]                         	 | INFO                                          |

## License

See [LICENSE](https://github.com/Timmi6790/s3-bucket-perma-link/blob/main/LICENSE.md) for
more information.
