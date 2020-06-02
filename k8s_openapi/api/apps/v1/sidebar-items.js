initSidebarItems({"struct":[["ControllerRevision","ControllerRevision implements an immutable snapshot of state data. Clients are responsible for serializing and deserializing the objects that contain their internal state. Once a ControllerRevision has been successfully created, it can not be updated. The API Server will fail validation of all requests that attempt to mutate the Data field. ControllerRevisions may, however, be deleted. Note that, due to its use by both the DaemonSet and StatefulSet controllers for update and rollback, this object is beta. However, it may be subject to name and representation changes in future releases, and clients should not depend on its stability. It is primarily for internal use by controllers."],["DaemonSet","DaemonSet represents the configuration of a daemon set."],["DaemonSetCondition","DaemonSetCondition describes the state of a DaemonSet at a certain point."],["DaemonSetSpec","DaemonSetSpec is the specification of a daemon set."],["DaemonSetStatus","DaemonSetStatus represents the current status of a daemon set."],["DaemonSetUpdateStrategy","DaemonSetUpdateStrategy is a struct used to control the update strategy for a DaemonSet."],["Deployment","Deployment enables declarative updates for Pods and ReplicaSets."],["DeploymentCondition","DeploymentCondition describes the state of a deployment at a certain point."],["DeploymentSpec","DeploymentSpec is the specification of the desired behavior of the Deployment."],["DeploymentStatus","DeploymentStatus is the most recently observed status of the Deployment."],["DeploymentStrategy","DeploymentStrategy describes how to replace existing pods with new ones."],["ReplicaSet","ReplicaSet ensures that a specified number of pod replicas are running at any given time."],["ReplicaSetCondition","ReplicaSetCondition describes the state of a replica set at a certain point."],["ReplicaSetSpec","ReplicaSetSpec is the specification of a ReplicaSet."],["ReplicaSetStatus","ReplicaSetStatus represents the current status of a ReplicaSet."],["RollingUpdateDaemonSet","Spec to control the desired behavior of daemon set rolling update."],["RollingUpdateDeployment","Spec to control the desired behavior of rolling update."],["RollingUpdateStatefulSetStrategy","RollingUpdateStatefulSetStrategy is used to communicate parameter for RollingUpdateStatefulSetStrategyType."],["StatefulSet","StatefulSet represents a set of pods with consistent identities. Identities are defined as:  - Network: A single stable DNS and hostname.  - Storage: As many VolumeClaims as requested. The StatefulSet guarantees that a given network identity will always map to the same storage identity."],["StatefulSetCondition","StatefulSetCondition describes the state of a statefulset at a certain point."],["StatefulSetSpec","A StatefulSetSpec is the specification of a StatefulSet."],["StatefulSetStatus","StatefulSetStatus represents the current state of a StatefulSet."],["StatefulSetUpdateStrategy","StatefulSetUpdateStrategy indicates the strategy that the StatefulSet controller will use to perform updates. It includes any additional parameters necessary to perform the update for the indicated strategy."]]});