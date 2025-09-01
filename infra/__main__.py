"""An AWS Python Pulumi program"""
from pathlib import Path
from typing import Dict, Any

import pulumi
import pulumi_aws as aws
from dotenv import dotenv_values

#Setting up repos
repo_backend = aws.ecr.get_repository(name='trading/backend')
repo_frontend = aws.ecr.get_repository(name='trading/frontend')
repo_trader = aws.ecr.get_repository(name='trading/trader')

availability_zone = aws.config.region
tag_deploy='dev'

backend_image = aws.ecr.get_image(
    repository_name=repo_backend.name,
    image_tag=tag_deploy  # Ensure this tag exists in ECR
)

frontend_image = aws.ecr.get_image(
    repository_name=repo_frontend.name,
    image_tag=tag_deploy  # Ensure this tag exists in ECR
)

trader_image = aws.ecr.get_image(
    repository_name=repo_trader.name,
    image_tag=tag_deploy  # Ensure this tag exists in ECR
)

backend_image_name = f"{repo_backend.repository_url}@{backend_image.image_digest}"
frontend_image_name = f"{repo_frontend.repository_url}@{frontend_image.image_digest}"
trader_image_name =  f"{repo_trader.repository_url}@{trader_image.image_digest}"

app_cluster = aws.ecs.Cluster("app-cluster")

############################
# Network vpc, subnet

# Creating a VPC and a public subnet
app_vpc = aws.ec2.Vpc("app-vpc", cidr_block="172.31.0.0/16", enable_dns_hostnames=True)

# Private dns
dns_ns = aws.servicediscovery.PrivateDnsNamespace(
    "p2p-ns",
    name="p2p.local",
    vpc=app_vpc.id,
    description="Private DNS for ECS services",
)

# Mandatory vpc subnets
app_vpc_subnet = aws.ec2.Subnet(
    "app-vpc-subnet",
    cidr_block="172.31.0.0/20",
    availability_zone="eu-west-1a",
    vpc_id=app_vpc.id,
)

app_vpc_subnet_b = aws.ec2.Subnet(
    "app-vpc-subnet-b",
    cidr_block="172.31.16.0/20",
    availability_zone="eu-west-1b",
    vpc_id=app_vpc.id,
)

# Creating a gateway to the web for the VPC
app_gateway = aws.ec2.InternetGateway("app-gateway", vpc_id=app_vpc.id)

app_routetable = aws.ec2.RouteTable(
    "app-routetable",
    routes=[
        aws.ec2.RouteTableRouteArgs(
            cidr_block="0.0.0.0/0",
            gateway_id=app_gateway.id,
        )
    ],
    vpc_id=app_vpc.id,
)
subnets: list = [app_vpc_subnet.id, app_vpc_subnet_b.id]


############################
## Security configuration

# Associating our gateway with our VPC, to allow our app to communicate with the greater internet
app_routetable_association = aws.ec2.MainRouteTableAssociation(
    "app_routetable_association", route_table_id=app_routetable.id, vpc_id=app_vpc.id
)

# Creating a Security Group that restricts incoming traffic to HTTP
sg_all_open = aws.ec2.SecurityGroup(
    "security-group",
    vpc_id=app_vpc.id,
    description="Enables all access",
    ingress=[
        aws.ec2.SecurityGroupIngressArgs(
            protocol="tcp",
            from_port=0,
            to_port=65535,
            cidr_blocks=["0.0.0.0/0"],
        )
    ],
    egress=[
        aws.ec2.SecurityGroupEgressArgs(
            protocol="-1",
            from_port=0,
            to_port=0,
            cidr_blocks=["0.0.0.0/0"],
        )
    ],
)

# Creating an IAM role used by Fargate to execute all our services
app_exec_role = aws.iam.Role(
    "app-exec-role",
    assume_role_policy="""{
        "Version": "2012-10-17",
        "Statement": [
        {
            "Action": "sts:AssumeRole",
            "Principal": {
                "Service": "ecs-tasks.amazonaws.com"
            },
            "Effect": "Allow",
            "Sid": ""
        }]
    }""",
)

security_groups: list = [sg_all_open.id]

############################

# Policies for running roles

# Attaching execution permissions to the exec role
exec_policy_attachment = aws.iam.RolePolicyAttachment(
    "app-exec-policy",
    role=app_exec_role.name,
    policy_arn=aws.iam.ManagedPolicy.AMAZON_ECS_TASK_EXECUTION_ROLE_POLICY,
)

# Creating an IAM role used by Fargate to manage tasks
app_task_role = aws.iam.Role(
    "app-task-role",
    assume_role_policy="""{
        "Version": "2012-10-17",
        "Statement": [
        {
            "Action": "sts:AssumeRole",
            "Principal": {
                "Service": "ecs-tasks.amazonaws.com"
            },
            "Effect": "Allow",
            "Sid": ""
        }]
    }""",
)

# Attaching execution permissions to the task role
task_policy_attachment = aws.iam.RolePolicyAttachment(
    "app-access-policy",
    role=app_task_role.name,
    policy_arn=aws.iam.ManagedPolicy.AMAZON_ECS_FULL_ACCESS,
)

############################

logs_group_name = "trading-infra-log-group"

# Creating a Cloudwatch instance to store the logs that the ECS services produce
log_group = aws.cloudwatch.LogGroup(
    logs_group_name, retention_in_days=1, name=logs_group_name
)

# Setting up private dns service names
from tools import make_service, make_sd_service, make_alb, make_alb_questdb, make_service_questdb

backend_sd = make_sd_service(dns_ns, "backend")
questdb_sd = make_sd_service(dns_ns, "questdb")
frontend_sd = make_sd_service(dns_ns, "frontend")
trader_sd = make_sd_service(dns_ns, "trader")

#setting up environment variables
backend_hostname = pulumi.Output.concat("backend.", dns_ns.name)
questdb_hostname = pulumi.Output.concat("questdb.", dns_ns.name)

env = dotenv_values(".env")  # dict[str,str|None]
# overwritten env variables

# WS_SERVER_HOST must stay as an Output
env['WS_SERVER_HOST'] = backend_hostname.apply(lambda h: str(h))
env['QUESTDB_CONFIG'] = questdb_hostname.apply(
    lambda host: f"http::addr={host}:9000;username=admin;password=quest;retry_timeout=20000;"
)
#ENDPOINT_ZMQ="tcp://ml:5555"

ecs_env = [
    {"name": k, "value": v}
    for k, v in env.items()
]

#setting up questdb

#Setting up storge
'''
efs = aws.efs.FileSystem("questdb-efs")
# Mount target (asocia EFS a la VPC/subnet)
mount_target = aws.efs.MountTarget("questdb-efs-mount",
    file_system_id=efs.id,
    subnet_id=subnets[0],
    security_groups=security_groups,
)
mount_points = [
          {{
            "sourceVolume": "questdb-data",
            "containerPath": "/var/lib/questdb"
          }}
        ];


volumes=[{
        "name": "questdb-data",
        "efsVolumeConfiguration": {
            "fileSystemId": efs.id,
        },
    }]
'''

name = "questdb"
image_name = "questdb/questdb:8.2.3"
cpu = '1024'
memory = 2048
port = 9000

port_2 = 9009
port_mappings=[{"containerPort": port, "protocol": "tcp"},{"containerPort": port_2, "protocol": "tcp"} ]

(alb_questdb, _, load_balancers) = make_alb_questdb(name, app_vpc, subnets, security_groups, port, "HTTP")
(service, definition) = make_service_questdb(app_cluster, app_exec_role, app_task_role,
                name,
                subnets,
                security_groups,
                availability_zone,
                image_name,
                cpu,
                memory,
                port_mappings,
                ecs_env,
                logs_group_name,
                [],
                load_balancers,
                questdb_sd)

questdb_public_url = pulumi.Output.concat("http::addr=", alb_questdb.dns_name, ":9000;username=admin;password=quest;retry_timeout=20000;")
pulumi.export("questdb_public_url", questdb_public_url)

# setting up backend
name = "backend"
image_name = backend_image_name
cpu = '256'
memory = 1024
port = int(env.get('WS_SERVER_PORT'))
port_mappings=[{"containerPort": port, "protocol": "tcp"}]
make_service(app_cluster, app_exec_role, app_task_role,
                name,
                subnets,
                security_groups,
                availability_zone,
                image_name,
                cpu,
                memory,
                port_mappings,
                ecs_env,
                logs_group_name,
                [],
                [],
                backend_sd)

# setting up frontend
name = "frontend"
image_name = frontend_image_name
cpu = '256'
memory = 1024
port = int(env.get('FRONTEND_PORT'))

port_mappings=[{"containerPort": port, "protocol": "tcp"}]
(alb, _, load_balancers) = make_alb(name, app_vpc, subnets, security_groups, port, "HTTP")
make_service(app_cluster, app_exec_role, app_task_role,
                name,
                subnets,
                security_groups,
                availability_zone,
                image_name,
                cpu,
                memory,
                port_mappings,
                ecs_env,
                logs_group_name,
                [],
                load_balancers,
                frontend_sd)


# setting up frontend
name = "trader"
image_name = trader_image_name
cpu = '256'
memory = 1024
port = 3000

port_mappings=[{"containerPort": port, "protocol": "tcp"}]
(alb, _, load_balancers) = make_alb(name, app_vpc, subnets, security_groups, port, "HTTP")
make_service(app_cluster, app_exec_role, app_task_role,
                name,
                subnets,
                security_groups,
                availability_zone,
                image_name,
                cpu,
                memory,
                port_mappings,
                ecs_env,
                logs_group_name,
                [],
                load_balancers,
                frontend_sd)



frontend_public_url = pulumi.Output.concat("http://", alb.dns_name, ":8501")
pulumi.export("frontend_public_url", frontend_public_url)

