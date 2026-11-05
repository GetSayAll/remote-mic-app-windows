pub mod adpcm;
pub mod agent_access;
pub mod atvv;
pub mod frame;
pub mod macros;
pub mod mcp_config;
pub mod pcm;
pub mod pipeline;
pub mod portable_macros;
pub mod settings;
pub mod statistics;
pub mod transcripts;
pub mod voice;

pub use adpcm::{ImaAdpcmDecoder, NibbleOrder};
pub use agent_access::{
    create_agent_authorization, discard_agent_authorization, generate_agent_client_id,
    generate_agent_token, hash_agent_executable_path, hash_agent_token, is_valid_agent_client_id,
    is_valid_agent_integration_identifier, normalize_agent_client_name,
    require_agent_authorization, revoke_agent_authorization, token_hash_matches,
    validate_agent_access_state, AgentAccessError, AgentAccessState, AgentAuthorization,
    CreatedAgentAuthorization, AGENT_ACCESS_SCHEMA_VERSION, AGENT_SCOPE_TRANSCRIPTS_READ_ALL,
    AGENT_TOKEN_BYTES, MAX_AGENT_CLIENT_NAME_CHARS, MAX_AGENT_INTEGRATION_ID_CHARS,
};
pub use atvv::{AtvvCapabilities, AtvvCommand, AtvvControlEvent, AtvvError, AtvvUuids};
pub use frame::FrameAccumulator;
pub use macros::{
    effective_step_timeout, find_cycle, is_valid_macro_id, latest_definition, latest_definitions,
    merge_library, validate_definition, validate_http_url, validate_library, ApplicationWaitState,
    MacroAction, MacroChord, MacroDefinition, MacroIssue, MacroLibraryDocument, MacroModifier,
    MacroParameters, MacroStep, MACRO_LIBRARY_FORMAT_VERSION, MACRO_SCHEMA_VERSION,
    MAX_LIBRARY_BYTES,
};
pub use pcm::{normalize_gain_db, process_pcm};
pub use pipeline::{AtvvVoicePipeline, PipelineError, PipelineOutput};
pub use portable_macros::{
    export_portable_library, import_portable_library, PortableError, PORTABLE_MAX_BYTES,
};
pub use settings::{
    AppIconIdentifier, AppLanguage, AppSettings, ThemePreference, VoiceInputTool, VoiceTriggerMode,
};
pub use statistics::{
    DailyUsage, UsageStatistics, VoiceSessionUsageRecord, MAX_APPLICATION_NAME_CHARS,
    MAX_STAT_KEY_CHARS, MAX_VOICE_SESSION_RANKING,
};
pub use transcripts::{
    application_key, continuous_insertion, is_local_date_key, is_safe_application_key,
    read_archive, visible_records, TranscriptArchive, TranscriptDayFile, TranscriptRecord,
    TranscriptSource, TranscriptTextChange, CAPTURE_METHOD_VERSION, MAX_DAY_FILE_BYTES,
    MAX_RECORDS_PER_DAY, MAX_RECORDS_TOTAL, MAX_TRANSCRIPT_CHARS, RECENT_WINDOW_MS,
    TRANSCRIPT_FORMAT_VERSION, TRANSCRIPT_SCHEMA_VERSION,
};
pub use voice::{VoiceSession, VoiceSessionError, VoiceSessionEvent, VoiceSessionState};
