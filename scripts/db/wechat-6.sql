UPDATE wechat_delivery SET plan = json_set(plan, '$.files', json('[]'));
