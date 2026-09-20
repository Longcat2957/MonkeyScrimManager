"""SQL preserved from the frozen Rust notification_outbox repository."""

CLAIM_CANDIDATES = """
WITH claim_clock AS MATERIALIZED (
             SELECT clock_timestamp() AS current_time
         )
         SELECT notification.id
         FROM bot_notification_outbox AS notification
         CROSS JOIN claim_clock
         WHERE notification.delivered_at IS NULL
           AND notification.failed_at IS NULL
           AND notification.attempts < :p1
           AND notification.available_at <= claim_clock.current_time
           AND (
                notification.lease_until IS NULL
                OR notification.lease_until < claim_clock.current_time
           )
           AND (
                notification.notification_type NOT IN (:p3, :p4, :p5, :p6, :p7, :p8)
                OR NOT EXISTS (
                    SELECT 1
                    FROM bot_notification_outbox AS earlier
                    WHERE earlier.guild_id = notification.guild_id
                      AND earlier.delivered_at IS NULL
                      AND earlier.failed_at IS NULL
                      AND (
                          (
                              notification.notification_type IN (:p3, :p4, :p5)
                              AND earlier.notification_type IN (:p3, :p4, :p5)
                              AND earlier.payload ->> 'recruitmentId'
                                  = notification.payload ->> 'recruitmentId'
                          )
                          OR (
                              notification.notification_type IN (:p6, :p7, :p8)
                              AND earlier.notification_type IN (:p6, :p7, :p8)
                              AND earlier.payload ->> 'auctionRecruitmentId'
                                  = notification.payload ->> 'auctionRecruitmentId'
                          )
                      )
                      AND (earlier.created_at, earlier.id)
                          < (notification.created_at, notification.id)
                )
           )
         ORDER BY notification.available_at, notification.id
         LIMIT :p2
         FOR UPDATE OF notification SKIP LOCKED
"""

CLAIM_UPDATE = """
WITH claim_clock AS MATERIALIZED (
                 SELECT clock_timestamp() AS current_time
             )
             UPDATE bot_notification_outbox AS notification
             SET attempts = notification.attempts + 1,
                 first_attempt_at = COALESCE(
                     notification.first_attempt_at, claim_clock.current_time),
                 lease_owner = :p1,
                 lease_token = :p2,
                 lease_until = claim_clock.current_time + :p3
             FROM claim_clock
             WHERE notification.id = :p4
               AND notification.delivered_at IS NULL
               AND notification.failed_at IS NULL
               AND notification.attempts < :p5
             RETURNING notification.id, notification.guild_id, notification.channel_id,
                       notification.notification_type, notification.payload,
                       notification.discord_nonce, notification.attempts,
                       notification.lease_until
"""

LEASE = """
SELECT notification.notification_type
         FROM bot_notification_outbox AS notification
         WHERE notification.id = :p1
           AND notification.lease_owner = :p2
           AND notification.lease_token = :p3
           AND notification.lease_until >= clock_timestamp()
           AND notification.delivered_at IS NULL
           AND notification.failed_at IS NULL
         FOR UPDATE
"""

ACK = """
UPDATE bot_notification_outbox AS notification
         SET discord_message_id = :p1,
             delivered_at = clock_timestamp(),
             lease_owner = NULL,
             lease_token = NULL,
             lease_until = NULL
         WHERE notification.id = :p2
           AND notification.lease_owner = :p3
           AND notification.lease_token = :p4
           AND notification.lease_until >= clock_timestamp()
           AND notification.delivered_at IS NULL
           AND notification.failed_at IS NULL
         RETURNING notification.id, notification.guild_id, notification.channel_id,
                   notification.notification_type, notification.payload,
                   notification.discord_message_id, notification.delivered_at,
                   notification.attempts
"""

BIND_REGULAR = """
UPDATE recruitments
         SET channel_id = :p1,
             message_id = :p2,
             revision = CASE
                 WHEN message_id IS NULL THEN revision + 1
                 ELSE revision
             END,
             updated_at = CASE
                 WHEN message_id IS NULL THEN :p3
                 ELSE updated_at
             END
         WHERE guild_id = :p4
           AND id = :p5
           AND (
                message_id IS NULL
                OR (channel_id = :p1 AND message_id = :p2)
           )
         RETURNING revision, status
"""

BIND_AUCTION = """
UPDATE auction_recruitments
         SET channel_id = :p1,
             message_id = :p2,
             revision = CASE
                 WHEN message_id IS NULL THEN revision + 1
                 ELSE revision
             END,
             updated_at = CASE
                 WHEN message_id IS NULL THEN :p3
                 ELSE updated_at
             END
         WHERE guild_id = :p4
           AND id = :p5
           AND (
                message_id IS NULL
                OR (channel_id = :p1 AND message_id = :p2)
           )
         RETURNING revision, status
"""

PAYLOAD_REGULAR = """
SELECT jsonb_build_object(
             'schemaVersion', CAST(:p3 AS integer),
             'recruitmentId', recruitment.id::text,
             'snapshot', jsonb_build_object(
                 'recruitment', jsonb_build_object(
                     'id', recruitment.id::text,
                     'displayNumber', recruitment.display_number,
                     'title', recruitment.title,
                     'status', recruitment.status,
                     'targetCount', recruitment.target_count,
                     'revision', recruitment.revision,
                     'channelId', recruitment.channel_id,
                     'messageId', recruitment.message_id,
                     'createdById', recruitment.created_by,
                     'updatedAt', recruitment.updated_at
                 ),
                 'participants', COALESCE((
                     SELECT jsonb_agg(
                         jsonb_build_object(
                             'id', participant.user_id,
                             'displayName', member.display_name,
                             'preferredLanes', COALESCE((
                                 SELECT jsonb_agg(preferred.lane ORDER BY preferred.priority)
                                 FROM recruitment_preferred_lanes AS preferred
                                 WHERE preferred.guild_id = participant.guild_id
                                   AND preferred.recruitment_id = participant.recruitment_id
                                   AND preferred.user_id = participant.user_id
                             ), '[]'::jsonb)
                         )
                         ORDER BY participant.joined_at, participant.user_id
                     )
                     FROM recruitment_participants AS participant
                     JOIN guild_members AS member
                       ON member.guild_id = participant.guild_id
                      AND member.user_id = participant.user_id
                     WHERE participant.guild_id = recruitment.guild_id
                       AND participant.recruitment_id = recruitment.id
                       AND participant.status = 'JOINED'
                 ), '[]'::jsonb),
                 'links', jsonb_build_object(
                     'entry', format('/g/%s/recruitments/%s/entry',
                         recruitment.guild_id, recruitment.id)
                 )
             )
         )
         FROM recruitments AS recruitment
         WHERE recruitment.guild_id = :p1 AND recruitment.id = :p2
"""

PAYLOAD_AUCTION = """
SELECT jsonb_build_object(
             'schemaVersion', CAST(:p3 AS integer),
             'auctionRecruitmentId', recruitment.id::text,
             'snapshot', jsonb_build_object(
                 'recruitment', jsonb_build_object(
                     'id', recruitment.id::text,
                     'displayNumber', recruitment.display_number,
                     'title', recruitment.title,
                     'format', recruitment.format,
                     'rules', jsonb_build_object(
                         'hardFearless', recruitment.hard_fearless,
                         'semifinalBestOf', recruitment.semifinal_best_of,
                         'finalBestOf', recruitment.final_best_of
                     ),
                     'participantCount', recruitment.target_count,
                     'joinedParticipantCount', COALESCE((
                         SELECT count(*)
                         FROM auction_recruitment_participants AS joined
                         WHERE joined.guild_id = recruitment.guild_id
                           AND joined.recruitment_id = recruitment.id
                           AND joined.status = 'JOINED'
                     ), 0),
                     'teamCount', CASE recruitment.format
                         WHEN 'TEN_PERSON' THEN 2
                         WHEN 'TWENTY_PERSON' THEN 4
                     END,
                     'status', recruitment.status,
                     'revision', recruitment.revision,
                     'channelId', recruitment.channel_id,
                     'messageId', recruitment.message_id,
                     'createdById', recruitment.created_by,
                     'updatedAt', recruitment.updated_at,
                     'href', format('/g/%s/auction/recruitments/%s',
                         recruitment.guild_id, recruitment.id)
                 ),
                 'participants', COALESCE((
                     SELECT jsonb_agg(
                         jsonb_build_object(
                             'id', participant.user_id,
                             'displayName', member.display_name,
                             'declaredLane', participant.declared_lane
                         )
                         ORDER BY participant.joined_at, participant.user_id
                     )
                     FROM auction_recruitment_participants AS participant
                     JOIN guild_members AS member
                       ON member.guild_id = participant.guild_id
                      AND member.user_id = participant.user_id
                     WHERE participant.guild_id = recruitment.guild_id
                       AND participant.recruitment_id = recruitment.id
                       AND participant.status = 'JOINED'
                 ), '[]'::jsonb),
                 'links', jsonb_build_object(
                     'draft', CASE WHEN recruitment.status = 'CLOSED'
                         THEN format('/g/%s/auction/recruitments/%s/draft',
                         recruitment.guild_id, recruitment.id)
                         ELSE NULL
                     END
                 ),
                 'permissions', jsonb_build_object(
                     'canManageMembers', false,
                     'canClose', false,
                     'canSendParticipantConfirmationDms', false,
                     'canCancel', false
                 )
             )
         )
         FROM auction_recruitments AS recruitment
         WHERE recruitment.guild_id = :p1 AND recruitment.id = :p2
"""

NACK_LEASE = """
SELECT notification.attempts
         FROM bot_notification_outbox AS notification
         WHERE notification.id = :p1
           AND notification.lease_owner = :p2
           AND notification.lease_token = :p3
           AND notification.lease_until >= clock_timestamp()
           AND notification.delivered_at IS NULL
           AND notification.failed_at IS NULL
         FOR UPDATE
"""

NACK_RETRY = """
UPDATE bot_notification_outbox AS notification
                 SET available_at = clock_timestamp() + :p1,
                     last_error = :p2,
                     lease_owner = NULL,
                     lease_token = NULL,
                     lease_until = NULL
                 WHERE notification.id = :p3
                   AND notification.lease_owner = :p4
                   AND notification.lease_token = :p5
                   AND notification.lease_until >= clock_timestamp()
                   AND notification.delivered_at IS NULL
                   AND notification.failed_at IS NULL
                 RETURNING notification.available_at
"""

NACK_FAIL = """
UPDATE bot_notification_outbox AS notification
                 SET failed_at = clock_timestamp(),
                     failure_code = :p1,
                     last_error = :p2,
                     lease_owner = NULL,
                     lease_token = NULL,
                     lease_until = NULL
                 WHERE notification.id = :p3
                   AND notification.lease_owner = :p4
                   AND notification.lease_token = :p5
                   AND notification.lease_until >= clock_timestamp()
                   AND notification.delivered_at IS NULL
                   AND notification.failed_at IS NULL
"""

REAP = """
WITH claim_clock AS MATERIALIZED (
             SELECT clock_timestamp() AS current_time
         )
         UPDATE bot_notification_outbox AS notification
         SET failed_at = claim_clock.current_time,
             failure_code = 'MAX_ATTEMPTS',
             last_error = COALESCE(
                 NULLIF(notification.last_error, ''),
                 'notification delivery exhausted maximum attempts'
             ),
             lease_owner = NULL,
             lease_token = NULL,
             lease_until = NULL
         FROM claim_clock
         WHERE notification.delivered_at IS NULL
           AND notification.failed_at IS NULL
           AND notification.attempts >= :p1
           AND (
                notification.lease_until IS NULL
                OR notification.lease_until < claim_clock.current_time
           )
"""
