# Multi-User Kubernetes Deployment

This directory contains Kubernetes manifests for deploying vibe-kanban in multi-user mode.

## Prerequisites

1. **AWS EKS Cluster** with the following add-ons installed:
   - AWS Load Balancer Controller (for ALB Ingress)
   - AWS EFS CSI Driver (for shared storage)
   - External DNS (optional, for automatic DNS management)

2. **AWS Resources**:
   - EFS File System in the same VPC as your EKS cluster
   - ACM Certificate for TLS termination
   - RDS PostgreSQL instance (or self-managed PostgreSQL)

3. **ECR Repository** with the vibe-kanban-desktop image

## Configuration

Before deploying, update the following placeholder values:

### In `storageclass.yaml`:
- `${EFS_FILE_SYSTEM_ID}`: Your EFS file system ID (e.g., `fs-0123456789abcdef0`)

### In `ingress.yaml`:
- `${ACM_CERT_ARN}`: Your ACM certificate ARN
- `desktop.vibe-kanban.example.com`: Your actual domain name

### In `deployment.yaml`:
- `${ECR_REPO}`: Your ECR repository URL (e.g., `123456789012.dkr.ecr.us-west-2.amazonaws.com`)

### In `secrets.yaml`:
- `database-url`: PostgreSQL connection string
- `jwt-secret`: JWT validation secret (generate with `openssl rand -base64 32`)
- `config-encryption-key`: AES-256 encryption key (generate with `openssl rand -base64 32`)

## Deployment

### Using Kustomize

```bash
# Preview the manifests
kubectl kustomize k8s/multiuser/

# Apply the manifests
kubectl apply -k k8s/multiuser/
```

### Using kubectl directly

```bash
# Create namespace first
kubectl apply -f k8s/multiuser/namespace.yaml

# Apply in order (dependencies first)
kubectl apply -f k8s/multiuser/serviceaccount.yaml
kubectl apply -f k8s/multiuser/storageclass.yaml
kubectl apply -f k8s/multiuser/pvc.yaml
kubectl apply -f k8s/multiuser/secrets.yaml
kubectl apply -f k8s/multiuser/deployment.yaml
kubectl apply -f k8s/multiuser/service.yaml
kubectl apply -f k8s/multiuser/ingress.yaml
```

## Verification

```bash
# Check pod status
kubectl get pods -n vibe -l app=vibe-kanban-desktop

# Check service
kubectl get svc -n vibe vibe-kanban-desktop

# Check ingress
kubectl get ingress -n vibe vibe-kanban-desktop-ingress

# Check PVC
kubectl get pvc -n vibe vibe-kanban-workspaces

# View pod logs
kubectl logs -n vibe -l app=vibe-kanban-desktop -f
```

## Scaling

```bash
# Scale deployment
kubectl scale deployment vibe-kanban-desktop -n vibe --replicas=3
```

## Secrets Management

For production, consider using one of these options instead of plain Kubernetes secrets:

1. **AWS Secrets Manager + External Secrets Operator**
2. **HashiCorp Vault**
3. **Sealed Secrets**

## Architecture

```
                    +----------------+
                    |   AWS ALB      |
                    |  (TLS term.)   |
                    +-------+--------+
                            |
                    +-------v--------+
                    |   Ingress      |
                    +-------+--------+
                            |
                    +-------v--------+
                    |   Service      |
                    |  (ClusterIP)   |
                    +-------+--------+
                            |
          +-----------------+-----------------+
          |                                   |
  +-------v--------+                 +--------v-------+
  |    Pod 1       |                 |    Pod 2       |
  | (vibe-kanban)  |                 | (vibe-kanban)  |
  +-------+--------+                 +--------+-------+
          |                                   |
          +-----------------+-----------------+
                            |
                    +-------v--------+
                    |  EFS Volume    |
                    |  /workspaces   |
                    +----------------+
                            |
                    +-------v--------+
                    |  PostgreSQL    |
                    |    (RDS)       |
                    +----------------+
```
