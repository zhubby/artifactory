-- [INPUT]: 依赖业务表、通知事件表、项目签名默认方案和当前成员关系。
-- [OUTPUT]: 同事务记录业务事件，扫描异常并关闭失效的项目签名告警。
-- [POS]: 统一事件生产边界；触发器只保存白名单数据，不执行网络请求。
-- [PROTOCOL]: Update this header on change, then check AGENTS.md
CREATE TABLE notification_aggregate_versions (
    aggregate_key text PRIMARY KEY,
    version bigint NOT NULL DEFAULT 0
);
CREATE TABLE notification_incidents (
    kind text NOT NULL,
    resource_id uuid NOT NULL,
    epoch uuid NOT NULL DEFAULT gen_random_uuid(),
    open boolean NOT NULL DEFAULT true,
    recovery_samples integer NOT NULL DEFAULT 0,
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(kind,resource_id)
);
CREATE INDEX notification_incidents_open_resource ON notification_incidents(resource_id) WHERE open;
CREATE INDEX notification_inbox_retention ON notification_inbox(updated_at,id) WHERE NOT needs_action;
CREATE INDEX notification_events_retention ON notification_events(occurred_at,id)
    WHERE projected_at IS NOT NULL AND channels_routed_at IS NOT NULL;
CREATE INDEX notification_deliveries_retention ON notification_deliveries(created_at,id);
CREATE INDEX notification_deliveries_mail_outcome ON notification_deliveries(completed_at DESC,user_id)
    WHERE channel IN ('email','verification') AND status IN ('failed','succeeded');

CREATE FUNCTION notification_next_version(k text) RETURNS bigint LANGUAGE sql AS $$
    INSERT INTO notification_aggregate_versions(aggregate_key,version) VALUES(k,1)
    ON CONFLICT(aggregate_key) DO UPDATE SET version=notification_aggregate_versions.version+1
    RETURNING version
$$;

CREATE FUNCTION notification_context(t uuid,p uuid) RETURNS jsonb LANGUAGE sql STABLE AS $$
    SELECT jsonb_strip_nulls(jsonb_build_object(
        'tenant_name',(SELECT name FROM tenants WHERE id=t),
        'tenant_slug',(SELECT slug FROM tenants WHERE id=t),
        'project_name',(SELECT name FROM projects WHERE id=p),
        'project_slug',(SELECT slug FROM projects WHERE id=p)))
$$;

CREATE FUNCTION notification_admin_audience(t uuid,p uuid) RETURNS uuid[] LANGUAGE sql STABLE AS $$
    SELECT coalesce(array_agg(DISTINCT user_id),'{}'::uuid[]) FROM (
        SELECT user_id FROM tenant_members WHERE tenant_id=t AND role IN ('owner','admin')
        UNION SELECT user_id FROM project_members WHERE project_id=p AND role='admin'
    ) audience
$$;

CREATE FUNCTION notification_build_audience(b uuid) RETURNS uuid[] LANGUAGE sql STABLE AS $$
    SELECT coalesce(array_agg(DISTINCT uid) FILTER(WHERE uid IS NOT NULL),'{}'::uuid[])
    FROM (
        SELECT requested_by AS uid FROM builds WHERE id=b
        UNION SELECT s.user_id FROM notification_project_subscriptions s JOIN builds bld
          ON bld.project_id=s.project_id WHERE bld.id=b
    ) audience
$$;

CREATE FUNCTION notification_emit(
    typ text,cat text,sev text,t uuid,p uuid,rtype text,rid uuid,
    aggregate text,data jsonb,recipients uuid[],attn boolean,action_required boolean
) RETURNS void LANGUAGE plpgsql AS $$
DECLARE revision bigint;
BEGIN
    revision:=notification_next_version(aggregate);
    INSERT INTO notification_events(event_key,event_type,category,severity,tenant_id,project_id,
        resource_type,resource_id,aggregate_key,aggregate_version,payload,candidate_user_ids,attention,needs_action)
    VALUES(aggregate||':'||revision,typ,cat,sev,t,p,rtype,rid,aggregate,revision,data,
        coalesce(recipients,'{}'::uuid[]),attn,action_required);
    PERFORM pg_notify('artifactory_notifications','changed');
END $$;

CREATE FUNCTION notification_business_change() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE n jsonb; o jsonb; t uuid; p uuid; rid uuid; uid uuid; b record;
    status_value text; aggregate text; category_value text; resource_kind text;
    audience uuid[]; data jsonb; changed boolean;
BEGIN
    IF TG_OP='DELETE' THEN n:=to_jsonb(OLD); ELSE n:=to_jsonb(NEW); END IF;
    IF TG_OP='INSERT' THEN o:='{}'::jsonb; ELSE o:=to_jsonb(OLD); END IF;
    t:=nullif(n->>'tenant_id','')::uuid;
    p:=nullif(n->>'project_id','')::uuid;
    rid:=nullif(n->>'id','')::uuid;
    IF TG_TABLE_NAME='builds' THEN
        data:=notification_context(t,p)||jsonb_build_object('build_number',n->'number',
            'status',n->'status','build_status',n->'status','publish_status',n->'publish_status',
            'release_version',n->'release_version','release_id',n->'release_id');
        aggregate:='build:'||rid;
        IF TG_OP='INSERT' OR n->'status' IS DISTINCT FROM o->'status' THEN
            status_value:=n->>'status';
            PERFORM notification_emit('build.status.changed','build',
                CASE WHEN status_value IN ('failed','lost') THEN 'error' WHEN status_value='succeeded' THEN 'success'
                  WHEN status_value='needs_input' THEN 'warning' ELSE 'info' END,
                t,p,'build',rid,aggregate,data,notification_build_audience(rid),
                status_value IN ('needs_input','succeeded','failed','lost','canceled'),status_value='needs_input');
        END IF;
        IF TG_OP='UPDATE' AND n->'publish_status' IS DISTINCT FROM o->'publish_status'
            AND n->>'publish_status'<>'not_requested' THEN
            status_value:=n->>'publish_status';
            PERFORM notification_emit('release.publication.changed','release',
                CASE WHEN status_value='failed' THEN 'error' WHEN status_value='succeeded' THEN 'success' ELSE 'info' END,
                t,p,'build',rid,aggregate,data||jsonb_build_object('status',status_value),
                notification_build_audience(rid),status_value IN ('succeeded','failed'),false);
        END IF;
    ELSIF TG_TABLE_NAME='step_runs' THEN
        IF TG_OP='UPDATE' AND n->'status' IS NOT DISTINCT FROM o->'status' THEN RETURN NEW; END IF;
        SELECT bld.*,j.target INTO b FROM attempts a JOIN jobs j ON j.id=a.job_id
          JOIN builds bld ON bld.id=j.build_id WHERE a.id=(n->>'attempt_id')::uuid;
        IF FOUND THEN
            PERFORM notification_emit('build.phase.changed','build','info',b.tenant_id,b.project_id,'build',b.id,
                'build:'||b.id,notification_context(b.tenant_id,b.project_id)||jsonb_build_object(
                  'build_number',b.number,'status',b.status,'build_status',b.status,'publish_status',b.publish_status,
                  'phase',n->'phase','step_status',n->'status','target',b.target,
                  'attempt_id',n->'attempt_id','step_index',n->'step_index','generation',b.revision_count),
                notification_build_audience(b.id),false,b.status='needs_input');
        END IF;
    ELSIF TG_TABLE_NAME='image_executions' THEN
        IF TG_OP='UPDATE' AND n->'status' IS NOT DISTINCT FROM o->'status' THEN RETURN NEW; END IF;
        SELECT bld.* INTO b FROM jobs j JOIN builds bld ON bld.id=j.build_id WHERE j.id=(n->>'job_id')::uuid;
        IF FOUND THEN
            PERFORM notification_emit('build.phase.changed','build','info',t,p,'build',b.id,'build:'||b.id,
                notification_context(t,p)||jsonb_build_object('build_number',b.number,'status',b.status,
                  'build_status',b.status,'publish_status',b.publish_status,'phase','image',
                  'step_status',n->'status','execution_id',rid,'generation',b.revision_count),
                notification_build_audience(b.id),false,b.status='needs_input');
        END IF;
    ELSIF TG_TABLE_NAME IN ('tenant_members','project_members') THEN
        IF TG_OP='UPDATE' AND n->'role' IS NOT DISTINCT FROM o->'role' THEN RETURN NEW; END IF;
        uid:=(n->>'user_id')::uuid;
        status_value:=CASE TG_OP WHEN 'INSERT' THEN 'added' WHEN 'DELETE' THEN 'removed' ELSE 'role_changed' END;
        data:=jsonb_build_object('status',status_value,'role',n->'role','subject_user_id',uid,
            'scope',CASE WHEN p IS NULL THEN 'tenant' ELSE 'project' END);
        IF TG_OP='DELETE' THEN
            PERFORM notification_emit('permission.member.changed','permission','warning',NULL,NULL,'account_safe',uid,
              'permission:'||coalesce(p,t)||':'||uid,data,ARRAY[uid],true,false);
        ELSE
            PERFORM notification_emit('permission.member.changed','permission','info',t,p,
              CASE WHEN p IS NULL THEN 'tenant' ELSE 'project' END,coalesce(p,t),
              'permission:'||coalesce(p,t)||':'||uid,notification_context(t,p)||data,ARRAY[uid],true,false);
        END IF;
        audience:=array_remove(notification_admin_audience(t,p),uid);
        PERFORM notification_emit('permission.member.changed','permission','info',t,p,
            CASE WHEN p IS NULL THEN 'tenant_admin' ELSE 'project_admin' END,coalesce(p,t),
            'permission-admin:'||coalesce(p,t)||':'||uid,notification_context(t,p)||data,audience,true,false);
    ELSIF TG_TABLE_NAME='projects' THEN
        changed:=n->'pool_ids' IS DISTINCT FROM o->'pool_ids' OR n->'node_ids' IS DISTINCT FROM o->'node_ids'
            OR n->'node_access_mode' IS DISTINCT FROM o->'node_access_mode'
            OR n->'credential_id' IS DISTINCT FROM o->'credential_id'
            OR n->'source_credentials' IS DISTINCT FROM o->'source_credentials'
            OR n->'signing_profiles' IS DISTINCT FROM o->'signing_profiles';
        IF changed THEN
            p:=rid;
            PERFORM notification_emit('permission.resources.changed','permission','warning',t,p,'project_admin',p,
              'resources:'||p,notification_context(t,p)||jsonb_build_object('status','resources_changed'),
              notification_admin_audience(t,p),true,false);
        END IF;
    ELSIF TG_TABLE_NAME='nodes' THEN
        IF n->'revoked' IS DISTINCT FROM o->'revoked' AND n->>'revoked'='true' THEN
            PERFORM notification_emit('device.status.changed','device','warning',t,NULL,'device',rid,
              'device-revoked:'||rid,notification_context(t,NULL)||jsonb_build_object('status','revoked','resource_name',n->'name'),
              notification_admin_audience(t,NULL),true,false);
        END IF;
        IF TG_OP='UPDATE' AND n->'certificate_hash' IS DISTINCT FROM o->'certificate_hash'
          AND (n->>'certificate_expires_at')::timestamptz>clock_timestamp() THEN
            PERFORM notification_emit('device.status.changed','device','success',t,NULL,'device',rid,
              'node-certificate:'||rid||':'||(o->>'certificate_hash'),notification_context(t,NULL)||
              jsonb_build_object('status','certificate_renewed','resource_name',n->'name'),
              notification_admin_audience(t,NULL),false,false);
        END IF;
    ELSIF TG_TABLE_NAME='emissary_updates' THEN
        status_value:=n->>'status';
        IF n->'status' IS NOT DISTINCT FROM o->'status' OR status_value NOT IN ('failed','rolled_back','succeeded') THEN RETURN NEW; END IF;
        rid:=(n->>'node_id')::uuid;
        aggregate:='device-update:'||rid;
        IF status_value='succeeded' THEN
            SELECT e.needs_action INTO changed FROM notification_events e WHERE e.aggregate_key=aggregate
              ORDER BY e.aggregate_version DESC LIMIT 1;
            IF NOT coalesce(changed,false) THEN RETURN NEW; END IF;
        END IF;
        PERFORM notification_emit('device.update.changed','device',
          CASE WHEN status_value='succeeded' THEN 'success' ELSE 'error' END,t,NULL,'device',rid,
          aggregate,notification_context(t,NULL)||jsonb_build_object(
            'status',CASE WHEN status_value='succeeded' THEN 'resolved' ELSE status_value END,
            'resource_name',(SELECT name FROM nodes WHERE id=rid),'update_id',n->'id'),
          notification_admin_audience(t,NULL),status_value<>'succeeded',status_value<>'succeeded');
    ELSIF TG_TABLE_NAME IN ('image_credentials','signing_credentials','signing_versions','signing_profiles','credentials') THEN
        IF TG_OP='DELETE' AND TG_TABLE_NAME IN ('signing_versions','signing_profiles')
          AND NOT EXISTS(SELECT 1 FROM signing_credentials WHERE id=(n->>'credential_id')::uuid) THEN RETURN OLD; END IF;
        IF TG_TABLE_NAME IN ('signing_versions','signing_profiles') THEN rid:=(n->>'credential_id')::uuid; END IF;
        aggregate:='credential:'||rid;
        IF TG_OP='DELETE' THEN status_value:='removed';
        ELSIF n->'enabled' IS DISTINCT FROM o->'enabled' AND n->>'enabled'='false' THEN status_value:='disabled';
        ELSIF n->'enabled' IS DISTINCT FROM o->'enabled' AND n->>'enabled'='true' THEN
            SELECT e.needs_action INTO changed FROM notification_events e WHERE e.aggregate_key=aggregate
              ORDER BY e.aggregate_version DESC LIMIT 1;
            IF NOT coalesce(changed,false) THEN RETURN NEW; END IF;
            status_value:='resolved';
        ELSE RETURN NEW;
        END IF;
        PERFORM notification_emit('credential.status.changed','credential',
          CASE WHEN status_value='resolved' THEN 'success' ELSE 'warning' END,t,p,
          CASE WHEN p IS NULL THEN 'credential' ELSE 'project_admin' END,rid,
          aggregate,notification_context(t,p)||jsonb_build_object('status',status_value,
            'resource_name',coalesce(n->'name',to_jsonb('Signing certificate'::text))),
          notification_admin_audience(t,p),status_value<>'resolved',status_value='disabled');
    ELSIF TG_TABLE_NAME='users' THEN
        IF TG_OP='UPDATE' AND n->'password_hash' IS DISTINCT FROM o->'password_hash' THEN
            PERFORM notification_emit('account.security.changed','account','warning',NULL,NULL,'account_safe',rid,
              'account-security:'||rid,jsonb_build_object('status','password_changed'),ARRAY[rid],true,false);
        END IF;
    ELSIF TG_TABLE_NAME='api_tokens' THEN
        IF TG_OP='INSERT' THEN status_value:='token_created';
        ELSIF TG_OP='DELETE' OR (n->>'revoked'='true' AND n->'revoked' IS DISTINCT FROM o->'revoked') THEN status_value:='token_revoked';
        ELSE RETURN NEW;
        END IF;
        uid:=(n->>'user_id')::uuid;
        PERFORM notification_emit('account.security.changed','account','info',NULL,NULL,'account_safe',uid,
          'account-token:'||rid,jsonb_build_object('status',status_value),ARRAY[uid],true,false);
    ELSIF TG_TABLE_NAME='lifecycle_operations' THEN
        status_value:=n->>'state';
        IF n->'state' IS DISTINCT FROM o->'state' AND status_value IN ('succeeded','failed','canceled') THEN
            IF n->>'action'='delete' AND status_value='succeeded' THEN
                PERFORM notification_emit('workspace.lifecycle.changed','workspace','info',NULL,NULL,'account_safe',
                  (n->>'actor_id')::uuid,'workspace-result:'||rid,jsonb_build_object('status',status_value,'action','delete'),
                  ARRAY[(n->>'actor_id')::uuid],true,false);
            END IF;
            audience:=notification_admin_audience(t,p);
            IF NOT ((n->>'actor_id')::uuid=ANY(audience)) THEN audience:=array_append(audience,(n->>'actor_id')::uuid); END IF;
            PERFORM notification_emit('workspace.lifecycle.changed','workspace',
              CASE WHEN status_value='failed' THEN 'error' ELSE 'info' END,t,p,
              CASE WHEN p IS NULL THEN 'tenant' ELSE 'project' END,coalesce(p,t),
              'workspace:'||rid,notification_context(t,p)||jsonb_build_object('status',status_value,'action',n->'action'),
              audience,true,status_value='failed');
        END IF;
    END IF;
    IF TG_OP='DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END $$;

CREATE TRIGGER notifications_build AFTER INSERT OR UPDATE OF status,publish_status ON builds
    FOR EACH ROW EXECUTE FUNCTION notification_business_change();
CREATE TRIGGER notifications_step AFTER INSERT OR UPDATE OF status ON step_runs
    FOR EACH ROW EXECUTE FUNCTION notification_business_change();
CREATE TRIGGER notifications_image AFTER INSERT OR UPDATE OF status ON image_executions
    FOR EACH ROW EXECUTE FUNCTION notification_business_change();
CREATE TRIGGER notifications_tenant_member AFTER INSERT OR UPDATE OF role OR DELETE ON tenant_members
    FOR EACH ROW EXECUTE FUNCTION notification_business_change();
CREATE TRIGGER notifications_project_member AFTER INSERT OR UPDATE OF role OR DELETE ON project_members
    FOR EACH ROW EXECUTE FUNCTION notification_business_change();
CREATE TRIGGER notifications_project_access AFTER UPDATE OF pool_ids,node_ids,node_access_mode,credential_id,source_credentials,signing_profiles ON projects
    FOR EACH ROW EXECUTE FUNCTION notification_business_change();
CREATE TRIGGER notifications_node AFTER UPDATE OF revoked,certificate_hash,certificate_expires_at ON nodes
    FOR EACH ROW EXECUTE FUNCTION notification_business_change();
CREATE TRIGGER notifications_update AFTER UPDATE OF status ON emissary_updates
    FOR EACH ROW EXECUTE FUNCTION notification_business_change();
CREATE TRIGGER notifications_image_credential AFTER UPDATE OF enabled OR DELETE ON image_credentials
    FOR EACH ROW EXECUTE FUNCTION notification_business_change();
CREATE TRIGGER notifications_signing_credential AFTER UPDATE OF enabled OR DELETE ON signing_credentials
    FOR EACH ROW EXECUTE FUNCTION notification_business_change();
CREATE TRIGGER notifications_signing_version AFTER UPDATE OF enabled OR DELETE ON signing_versions
    FOR EACH ROW EXECUTE FUNCTION notification_business_change();
CREATE TRIGGER notifications_signing_profile AFTER UPDATE OF enabled OR DELETE ON signing_profiles
    FOR EACH ROW EXECUTE FUNCTION notification_business_change();
CREATE TRIGGER notifications_source_credential AFTER DELETE ON credentials
    FOR EACH ROW EXECUTE FUNCTION notification_business_change();
CREATE TRIGGER notifications_account AFTER UPDATE OF password_hash ON users
    FOR EACH ROW EXECUTE FUNCTION notification_business_change();
CREATE TRIGGER notifications_token AFTER INSERT OR UPDATE OF revoked OR DELETE ON api_tokens
    FOR EACH ROW EXECUTE FUNCTION notification_business_change();
CREATE TRIGGER notifications_lifecycle AFTER UPDATE OF state ON lifecycle_operations
    FOR EACH ROW EXECUTE FUNCTION notification_business_change();

-- 异常扫描状态持久化。副本重启不重新制造同一告警。
CREATE FUNCTION notification_scan_anomalies() RETURNS integer LANGUAGE plpgsql AS $$
DECLARE n record; c record; incident notification_incidents; bucket text; processed integer:=0; prior record;
    is_offline boolean; is_online boolean; eligible boolean; recips uuid[]; project record;
BEGIN
    IF NOT pg_try_advisory_xact_lock(hashtextextended(current_database()||':'||current_schema()||':notification-scan',70236112303)) THEN RETURN 0; END IF;
    FOR n IN SELECT nd.*,tn.status AS tenant_status FROM nodes nd JOIN tenants tn ON tn.id=nd.tenant_id
    LOOP
        eligible:=n.tenant_status='active' AND NOT n.revoked AND NOT n.draining AND NOT n.ota_draining
          AND n.certificate_expires_at>clock_timestamp();
        is_offline:=eligible AND n.last_seen<clock_timestamp()-interval '120 seconds';
        is_online:=eligible AND n.last_seen>clock_timestamp()-interval '30 seconds'
          AND n.active_session IS NOT NULL AND n.connection_owner IS NOT NULL;
        SELECT * INTO incident FROM notification_incidents WHERE kind='device_offline' AND resource_id=n.id FOR UPDATE;
        IF is_offline THEN
            IF NOT FOUND OR NOT incident.open THEN
                INSERT INTO notification_incidents(kind,resource_id,epoch,open,recovery_samples)
                  VALUES('device_offline',n.id,gen_random_uuid(),true,0)
                  ON CONFLICT(kind,resource_id) DO UPDATE SET epoch=gen_random_uuid(),open=true,recovery_samples=0,
                    updated_at=clock_timestamp() RETURNING * INTO incident;
                PERFORM notification_emit('device.status.changed','device','error',n.tenant_id,NULL,'device',n.id,
                  'device-incident:'||incident.epoch,notification_context(n.tenant_id,NULL)||
                  jsonb_build_object('status','offline','resource_name',n.name),notification_admin_audience(n.tenant_id,NULL),true,true);
                -- 构建发起人只看到自己的构建摘要，不获得设备运维明细。
                FOR project IN SELECT DISTINCT b.id,b.project_id,b.number,b.requested_by,b.status,b.publish_status FROM attempts a
                  JOIN jobs j ON j.id=a.job_id JOIN builds b ON b.id=j.build_id
                  WHERE a.node_id=n.id AND a.status IN ('leased','running') AND b.requested_by IS NOT NULL
                LOOP
                    PERFORM notification_emit('build.phase.changed','build','warning',n.tenant_id,project.project_id,'build',project.id,
                      'build:'||project.id,notification_context(n.tenant_id,project.project_id)||
                      jsonb_build_object('status',project.status,'build_status',project.status,'publish_status',project.publish_status,'reason_code','device_unavailable','build_number',project.number),notification_build_audience(project.id),true,project.status='needs_input');
                END LOOP;
                processed:=processed+1;
            ELSE
                UPDATE notification_incidents SET recovery_samples=0,updated_at=clock_timestamp()
                  WHERE kind='device_offline' AND resource_id=n.id;
            END IF;
        ELSIF FOUND AND incident.open THEN
            IF is_online THEN
                UPDATE notification_incidents SET recovery_samples=recovery_samples+1,updated_at=clock_timestamp()
                  WHERE kind='device_offline' AND resource_id=n.id RETURNING * INTO incident;
                IF incident.recovery_samples>=2 THEN
                    UPDATE notification_incidents SET open=false WHERE kind='device_offline' AND resource_id=n.id;
                    PERFORM notification_emit('device.status.changed','device','success',n.tenant_id,NULL,'device',n.id,
                      'device-incident:'||incident.epoch,notification_context(n.tenant_id,NULL)||
                      jsonb_build_object('status','online','resource_name',n.name),notification_admin_audience(n.tenant_id,NULL),false,false);
                    processed:=processed+1;
                END IF;
            ELSIF NOT eligible THEN
                UPDATE notification_incidents SET open=false,recovery_samples=0,updated_at=clock_timestamp()
                  WHERE kind='device_offline' AND resource_id=n.id;
                PERFORM notification_emit('device.status.changed','device','info',n.tenant_id,NULL,'device',n.id,
                  'device-incident:'||incident.epoch,notification_context(n.tenant_id,NULL)||
                  jsonb_build_object('status','monitoring_suspended','resource_name',n.name),
                  notification_admin_audience(n.tenant_id,NULL),false,false);
            ELSE
                UPDATE notification_incidents SET recovery_samples=0 WHERE kind='device_offline' AND resource_id=n.id;
            END IF;
        END IF;
        IF n.tenant_status='active' AND NOT n.revoked AND n.certificate_expires_at<=clock_timestamp() THEN
            INSERT INTO notification_incidents(kind,resource_id,open) VALUES('node_certificate:'||n.certificate_hash,n.id,true)
              ON CONFLICT DO NOTHING;
            IF FOUND THEN
                PERFORM notification_emit('device.status.changed','device','error',n.tenant_id,NULL,'device',n.id,
                  'node-certificate:'||n.id||':'||n.certificate_hash,notification_context(n.tenant_id,NULL)||
                  jsonb_build_object('status','certificate_expired','resource_name',n.name),notification_admin_audience(n.tenant_id,NULL),true,true);
                processed:=processed+1;
            END IF;
        END IF;
    END LOOP;
    FOR c IN SELECT cred.id,cred.name,cred.tenant_id,v.id AS version_id,
      (v.metadata->>'not_after')::bigint AS expires FROM signing_credentials cred
      JOIN tenants tn ON tn.id=cred.tenant_id AND tn.status='active'
      JOIN LATERAL(SELECT id,metadata,enabled FROM signing_versions WHERE credential_id=cred.id
        ORDER BY created_at DESC,id DESC LIMIT 1) v ON true
      WHERE cred.enabled AND v.enabled AND v.metadata->>'not_after' ~ '^[0-9]{1,18}$'
        AND (v.metadata->>'not_after')::bigint<=extract(epoch FROM clock_timestamp()+interval '7 days')
    LOOP
        bucket:=CASE WHEN c.expires<=extract(epoch FROM clock_timestamp()) THEN 'expired'
          WHEN c.expires<=extract(epoch FROM clock_timestamp()+interval '1 day') THEN 'expiring_1d' ELSE 'expiring_7d' END;
        INSERT INTO notification_incidents(kind,resource_id,open) VALUES('signing:'||bucket,c.version_id,true)
          ON CONFLICT(kind,resource_id) DO UPDATE SET open=true,updated_at=clock_timestamp()
            WHERE NOT notification_incidents.open;
        IF FOUND THEN
            recips:=notification_admin_audience(c.tenant_id,NULL);
            PERFORM notification_emit('credential.status.changed','credential',
              CASE WHEN bucket='expired' THEN 'error' ELSE 'warning' END,c.tenant_id,NULL,'credential',c.id,
              'signing:'||c.version_id,notification_context(c.tenant_id,NULL)||
              jsonb_build_object('status',bucket,'resource_name',c.name),recips,true,true);
            processed:=processed+1;
        END IF;
        -- 项目绑定独立于证书周期；切换默认方案后再次绑定会重新预警。
        FOR project IN SELECT DISTINCT pr.id FROM projects pr JOIN signing_profiles sp
          ON sp.tenant_id=pr.tenant_id AND sp.name IN (SELECT value FROM jsonb_each_text(pr.signing_defaults))
          WHERE sp.credential_id=c.id AND sp.enabled AND pr.status='active'
            AND (SELECT e.payload->>'status' FROM notification_events e
              WHERE e.aggregate_key='signing-project:'||pr.id||':'||c.version_id
              ORDER BY e.aggregate_version DESC LIMIT 1) IS DISTINCT FROM bucket
        LOOP
            PERFORM notification_emit('credential.status.changed','credential',
              CASE WHEN bucket='expired' THEN 'error' ELSE 'warning' END,c.tenant_id,project.id,'project_admin',project.id,
              'signing-project:'||project.id||':'||c.version_id,
              notification_context(c.tenant_id,project.id)||jsonb_build_object('status',bucket,'resource_name',c.name),
              notification_admin_audience(c.tenant_id,project.id),true,true);
        END LOOP;
    END LOOP;
    -- 项目不再使用该证书时关闭项目告警，租户证书告警保持独立。
    FOR prior IN SELECT e.* FROM (
      SELECT DISTINCT ON(aggregate_key) * FROM notification_events
      WHERE event_type='credential.status.changed' AND aggregate_key LIKE 'signing-project:%'
      ORDER BY aggregate_key,aggregate_version DESC) e
      WHERE e.needs_action AND NOT EXISTS(
        SELECT 1 FROM projects pr JOIN signing_profiles sp ON sp.tenant_id=pr.tenant_id
          AND sp.name IN (SELECT value FROM jsonb_each_text(pr.signing_defaults))
        JOIN signing_credentials sc ON sc.id=sp.credential_id
        JOIN LATERAL(SELECT id,enabled FROM signing_versions WHERE credential_id=sc.id
          ORDER BY created_at DESC,id DESC LIMIT 1) v ON true
        WHERE pr.id=e.project_id AND pr.status='active' AND sp.enabled AND sc.enabled AND v.enabled
          AND e.aggregate_key='signing-project:'||pr.id||':'||v.id)
    LOOP
        PERFORM notification_emit(prior.event_type,prior.category,'info',prior.tenant_id,prior.project_id,
          prior.resource_type,prior.resource_id,prior.aggregate_key,prior.payload||jsonb_build_object('status','resolved'),
          notification_admin_audience(prior.tenant_id,prior.project_id),false,false);
    END LOOP;
    -- 替换或停用版本后关闭旧版本告警，排队邮件据此取消过时提醒。
    FOR c IN SELECT i.kind,i.resource_id FROM notification_incidents i
      LEFT JOIN signing_versions v ON v.id=i.resource_id LEFT JOIN signing_credentials sc ON sc.id=v.credential_id
      WHERE i.kind LIKE 'signing:%' AND i.open AND (v.id IS NULL OR sc.id IS NULL OR NOT v.enabled OR NOT sc.enabled OR
        i.resource_id IS DISTINCT FROM (SELECT id FROM signing_versions WHERE credential_id=sc.id ORDER BY created_at DESC,id DESC LIMIT 1))
    LOOP
        UPDATE notification_incidents SET open=false,updated_at=clock_timestamp() WHERE kind=c.kind AND resource_id=c.resource_id;
        FOR prior IN SELECT DISTINCT ON(e.aggregate_key) e.* FROM notification_events e
          WHERE e.event_type='credential.status.changed' AND
            (e.aggregate_key='signing:'||c.resource_id OR e.aggregate_key LIKE 'signing-project:%:'||c.resource_id)
          ORDER BY e.aggregate_key,e.aggregate_version DESC
        LOOP
            PERFORM notification_emit(prior.event_type,prior.category,'info',prior.tenant_id,prior.project_id,
              prior.resource_type,prior.resource_id,prior.aggregate_key,prior.payload||jsonb_build_object('status','resolved'),
              prior.candidate_user_ids,false,false);
        END LOOP;
    END LOOP;
    FOR n IN SELECT * FROM notification_webhooks LOOP
        SELECT * INTO incident FROM notification_incidents WHERE kind='channel_failure' AND resource_id=n.id FOR UPDATE;
        IF n.last_status IN ('failed','paused') AND (NOT FOUND OR NOT incident.open) THEN
            INSERT INTO notification_incidents(kind,resource_id,epoch,open) VALUES('channel_failure',n.id,gen_random_uuid(),true)
              ON CONFLICT(kind,resource_id) DO UPDATE SET epoch=gen_random_uuid(),open=true,updated_at=clock_timestamp()
              RETURNING * INTO incident;
            PERFORM notification_emit('delivery.channel.changed','delivery','error',n.tenant_id,n.project_id,
              CASE WHEN n.project_id IS NULL THEN 'tenant_admin' ELSE 'project_admin' END,n.id,
              'delivery:'||incident.epoch,notification_context(n.tenant_id,n.project_id)||jsonb_build_object('status','failed','resource_name',n.name),
              notification_admin_audience(n.tenant_id,n.project_id),true,true);
        ELSIF FOUND AND incident.open AND n.last_status='succeeded' THEN
            UPDATE notification_incidents SET open=false,updated_at=clock_timestamp() WHERE kind='channel_failure' AND resource_id=n.id;
            PERFORM notification_emit('delivery.channel.changed','delivery','success',n.tenant_id,n.project_id,
              CASE WHEN n.project_id IS NULL THEN 'tenant_admin' ELSE 'project_admin' END,n.id,
              'delivery:'||incident.epoch,notification_context(n.tenant_id,n.project_id)||jsonb_build_object('status','resolved','resource_name',n.name),
              notification_admin_audience(n.tenant_id,n.project_id),false,false);
        END IF;
    END LOOP;
    -- 端点删除后，保留当前告警的安全摘要并关闭待处理周期。
    FOR prior IN SELECT DISTINCT ON(e.aggregate_key) e.* FROM notification_events e
      JOIN notification_incidents i ON i.kind='channel_failure' AND i.resource_id=e.resource_id
        AND i.open AND e.aggregate_key='delivery:'||i.epoch
      WHERE e.event_type='delivery.channel.changed' AND NOT EXISTS(SELECT 1 FROM notification_webhooks w WHERE w.id=e.resource_id)
      ORDER BY e.aggregate_key,e.aggregate_version DESC
    LOOP
        UPDATE notification_incidents SET open=false,updated_at=clock_timestamp()
          WHERE kind='channel_failure' AND resource_id=prior.resource_id;
        PERFORM notification_emit(prior.event_type,prior.category,'info',prior.tenant_id,prior.project_id,
          prior.resource_type,prior.resource_id,prior.aggregate_key,prior.payload||jsonb_build_object('status','resolved'),
          notification_admin_audience(prior.tenant_id,prior.project_id),false,false);
    END LOOP;
    -- 邮箱验证或业务邮件失败时，账户本人能在站内发现问题。
    FOR c IN SELECT DISTINCT ON(user_id) user_id,status FROM notification_deliveries
      WHERE channel IN ('email','verification') AND status IN ('failed','succeeded') AND completed_at>now()-interval '1 day'
      ORDER BY user_id,completed_at DESC,id DESC
    LOOP
        SELECT * INTO incident FROM notification_incidents WHERE kind='email_failure' AND resource_id=c.user_id FOR UPDATE;
        IF c.status='failed' AND (NOT FOUND OR NOT incident.open) THEN
            INSERT INTO notification_incidents(kind,resource_id,epoch,open) VALUES('email_failure',c.user_id,gen_random_uuid(),true)
              ON CONFLICT(kind,resource_id) DO UPDATE SET epoch=gen_random_uuid(),open=true,updated_at=clock_timestamp() RETURNING * INTO incident;
            PERFORM notification_emit('delivery.channel.changed','delivery','error',NULL,NULL,'account_safe',c.user_id,
              'delivery:'||incident.epoch,jsonb_build_object('status','failed'),ARRAY[c.user_id],true,true);
        ELSIF FOUND AND incident.open AND c.status='succeeded' THEN
            UPDATE notification_incidents SET open=false,updated_at=clock_timestamp() WHERE kind='email_failure' AND resource_id=c.user_id;
            PERFORM notification_emit('delivery.channel.changed','delivery','success',NULL,NULL,'account_safe',c.user_id,
              'delivery:'||incident.epoch,jsonb_build_object('status','resolved'),ARRAY[c.user_id],false,false);
        END IF;
    END LOOP;
    RETURN processed;
END $$;

-- 资源删除在同一事务清除通知正文和待发任务，不依赖定时清扫。
CREATE FUNCTION notification_cleanup_resource() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    -- 投影器先锁事件再写收件箱；删除保持相同锁顺序。
    PERFORM id FROM notification_events WHERE
      (TG_TABLE_NAME='projects' AND project_id=OLD.id) OR (TG_TABLE_NAME='tenants' AND tenant_id=OLD.id)
      ORDER BY occurred_at,aggregate_version,id FOR UPDATE;
    DELETE FROM notification_deliveries d USING notification_events e WHERE d.event_id=e.id AND
      ((TG_TABLE_NAME='projects' AND e.project_id=OLD.id) OR (TG_TABLE_NAME='tenants' AND e.tenant_id=OLD.id));
    DELETE FROM notification_inbox WHERE
      (TG_TABLE_NAME='projects' AND project_id=OLD.id) OR (TG_TABLE_NAME='tenants' AND tenant_id=OLD.id);
    DELETE FROM notification_events WHERE
      (TG_TABLE_NAME='projects' AND project_id=OLD.id) OR (TG_TABLE_NAME='tenants' AND tenant_id=OLD.id);
    RETURN OLD;
END $$;
CREATE TRIGGER notifications_cleanup_project AFTER DELETE ON projects FOR EACH ROW EXECUTE FUNCTION notification_cleanup_resource();
CREATE TRIGGER notifications_cleanup_tenant AFTER DELETE ON tenants FOR EACH ROW EXECUTE FUNCTION notification_cleanup_resource();
