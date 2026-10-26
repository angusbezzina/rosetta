import * as pulumi from "@pulumi/pulumi";
import * as aws from "@pulumi/aws";

const config = new pulumi.Config();
export const assets = new aws.s3.Bucket("assets", {});
export const api = new aws.lambda.Function("api", { role: config.require("role") });
export const invoke = new aws.lambda.Permission("invoke", {});
