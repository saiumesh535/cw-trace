import * as cdk from 'aws-cdk-lib/core';
import { Construct } from 'constructs';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import * as logs from 'aws-cdk-lib/aws-logs';
// import * as sqs from 'aws-cdk-lib/aws-sqs';

export class CdkCodeStack extends cdk.Stack {
  constructor(scope: Construct, id: string, props?: cdk.StackProps) {
    super(scope, id, props);

    // Minimal demo resource for cw-trace testing: explicit function name +
    // explicit log group, so the construct → log-group map resolves cleanly.
    // Passing `logGroup` stops the Lambda construct from auto-creating its
    // own (cdk.json enables useCdkManagedLogGroup, which would collide).
    const logGroup = new logs.LogGroup(this, 'CreateOrderLogGroup', {
      logGroupName: '/aws/lambda/sample-create-order',
      retention: logs.RetentionDays.ONE_MONTH,
      removalPolicy: cdk.RemovalPolicy.DESTROY,
    });

    new lambda.Function(this, 'CreateOrder', {
      functionName: 'sample-create-order',
      runtime: lambda.Runtime.NODEJS_22_X,
      handler: 'index.handler',
      code: lambda.Code.fromInline(
        'exports.handler = async (event) => ({ statusCode: 200, body: "ok" });',
      ),
      logGroup,
    });

    // example resource
    // const queue = new sqs.Queue(this, 'CdkCodeQueue', {
    //   visibilityTimeout: cdk.Duration.seconds(300)
    // });
  }
}
