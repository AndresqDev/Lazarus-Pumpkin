//! Port of Lazarus `language.yml` (curated subset used by the ported modules).
//!
//! Messages still use `&`-style legacy formatting and `<placeholder>` tokens,
//! exactly like the original, so they can be dropped in byte-for-byte by server
//! owners. `utils::text` parses the formatting and `utils::replace` fills tokens.

use serde::{Deserialize, Serialize};

macro_rules! lang {
    ($($field:ident = $default:expr;)*) => {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[serde(default)]
        pub struct Lang {
            $(pub $field: String,)*
        }

        impl Default for Lang {
            fn default() -> Self {
                Self { $($field: $default.to_string(),)* }
            }
        }
    };
}

lang! {
    prefix = "";
    faction_prefix = "";
    yes = "Yes";
    no = "No";
    none = "None";

    for_players_only = "&eThis command is for &cplayer &euse only!";
    for_console_only = "&eThis command is for &cconsole &euse only!";
    no_permission = "&eYou have no &cpermission &efor this command!";
    player_not_online = "&ePlayer &c<player> &eis not online!";
    player_not_found = "&ePlayer &c<player> &ehas never played before!";
    command_not_found = "&eCommand &c<command> &ecould not be found!";
    invalid_number = "&ePlease use a valid &cinteger &evalue.";
    invalid_duration = "&ePlease use a valid &ctime duration&e.";
    command_cooldown = "&cYou are currently on command cooldown.";

    join_welcome = "&eWelcome <player> &eto &6HCF&e!";
    staff_join = "&8[&aJoin&8] &a<player>";
    staff_quit = "&8[&cQuit&8] &c<player>";

    // ---- Factions (exact port of language.yml FACTIONS) ----
    faction_blocked_name = "&eThis faction name is &cforbidden&e!";
    faction_name_too_short = "&eName did not reach &cminimum &efaction name length!";
    faction_name_too_long = "&eName exceeded &cmaximum &efaction name length!";
    faction_already_exists = "&eFaction with name &c<name> &ealready exists!";
    faction_doesnt_exist = "&eFaction with search argument &c<argument> &edoesn't exist!";
    faction_already_in_self = "&eYou are already in &cfaction&e.";
    faction_already_in_others = "&ePlayer &c<player> &eis already in your faction.";
    faction_not_in = "&eYou are not in a &cfaction&e.";
    faction_not_in_player = "&ePlayer &c<player> &eis not in a faction.";
    faction_not_in_others = "&ePlayer &c<player> &eis not in your faction.";
    faction_leader_only = "&eYou must be faction &cleader&e!";
    faction_no_permission = "&eYou must be at least &c<role>&e.";
    faction_member_death = "&cMember &b<player> &cdied! &7(Loss: &c<dtrLoss>&7) &7(Dtr: <dtr>&7/<maxDtr>&7)";

    faction_created = "&7[&b*&7] &eFaction &a<name> &ewas created by &a<player>";
    faction_create_cooldown = "&eYou can create new &cfaction &ein &b<time>&e.";
    faction_disbanded = "&7[&b*&7] &eFaction &c<name> &ewas disbanded by &c<player>";
    faction_joined = "&ePlayer &c<player> &ehas joined faction.";
    faction_join_not_invited = "&eYou are not invited to faction &c<name>&e.";
    faction_full = "&eYour &cfaction &ealready has max member count.";
    faction_invited = "&eYou have been invited to faction &c<name>&e.";
    faction_invitation_received = "&eYou have been invited to faction &c<name>&e.";
    faction_invite_already = "&ePlayer &c<player> &eis already invited to faction.";
    faction_no_invitation = "&eYou are not invited to faction &c<name>&e.";
    faction_left = "&eYou have left faction &c<name>&e.";
    faction_leave_leader = "&eYou are &cfaction leader &eand therefore cannot leave this faction.";
    faction_member_joined = "&ePlayer &c<player> &ehas joined faction.";
    faction_member_left = "&ePlayer &c<player> &ehas left the faction.";
    faction_kicked = "&ePlayer &c<player> &ehas been kicked from faction.";
    faction_you_were_kicked = "&eYou have been kicked from faction &c<name>&e.";
    faction_promoted = "&c<player> &ehas been promoted to &c<role>&e.";
    faction_promote_max = "&ePlayer is already &chighest &erole.";
    faction_demoted = "&c<player> &ehas been demoted to &c<role>&e.";
    faction_demote_min = "&ePlayer is already &clowest &erole.";
    faction_leader_changed = "&c<player> &etransferred faction leadership to &c<target>&e.";
    faction_renamed = "&7[&b*&7] &eFaction &c<name> &ewas renamed to &a<newName>";
    faction_rename_same = "&eFaction already has name &c<name>&e.";
    faction_opened = "&7[&b*&7] &eFaction &c<name> &eis now &aopened &eto the public.";
    faction_closed = "&7[&b*&7] &eFaction &c<name> &eis now &cclosed &eto the public.";
    faction_open_cooldown = "&eYou can change your faction &copen status &ein &b<time>&e!";
    faction_open_changed = "&eYour faction open status changed.";
    faction_chat_toggled = "&eChat type changed to &c<type>&e.";

    faction_home_set = "&eYour &cfaction home &ehas been set to &b<x>&e, &b<z> &eby &c<player>&e.";
    faction_home_not_in_own = "&cFaction home &emust be in your territory.";
    faction_home_not_set = "&eYour &cfaction home &eis not set.";
    faction_home_teleport = "&eYou will be teleported to &cfaction home &ein &b<seconds> &eseconds.";
    faction_home_teleported = "&eYou have been teleported to your &cfaction home&e.";
    faction_home_teleport_cancelled = "&cHome teleport &ecancelled because you &cmoved&e!";
    faction_home_in_combat = "&eYou cannot teleport to &cfaction home &ewhile in &ccombat&e.";
    faction_home_in_enemy = "&eYou cannot teleport to &cfaction home &ewhile in &cenemy territory&e.";
    faction_home_with_pvp = "&eYou cannot teleport to &cfaction home &ewith enabled &cpvp timer&e.";

    faction_claim_created = "&c<player> &ehas claimed land between: &b<location1> &eand &b<location2>&e.";
    faction_claim_removed = "&c<player> &ehas unclaimed faction &cclaim &eat &b<location>&e.";
    faction_claim_not_owned = "&eYour faction does not own this &cclaim&e.";
    faction_claim_too_small = "&eClaim must be at least &c<size> blocks &ewide.";
    faction_claim_too_large = "&eClaim max size is &c<size> blocks &ewide.";
    faction_claim_not_connected = "&eCurrent claim selection is &cnot connected &eto your existing claims!";
    faction_claim_limit = "&eYour faction has exceeded maximum &cclaim count&e.";
    faction_claim_overlap = "&eThis claim selection is &coverlapping &ewith other claims.";
    faction_claim_not_enough_money = "&eYour &cfaction &edoes not have enough money!";
    faction_not_enough_money = "&eYour &cfaction &edoes not have enough money!";
    faction_claim_wand_received = "&eClaiming wand added to inventory, read the item to understand how to claim. Alternatively you can use &b/f claimchunk&e.";
    faction_claim_selection_cleared = "&eYou have cleared your current &cclaim selection&e.";
    faction_claim_selection_not_set = "&ePlease finish making &cclaim selection &ebefore claiming.";
    faction_claim_position_one = "&eSet &cfirst &eclaim position. (&b<location>&e)";
    faction_claim_position_two = "&eSet &csecond &eclaim position. (&b<location>&e)";
    faction_claim_price_message = "&bClaim selection cost: <price>&b. Current size: (&f<sizeX>, <sizeZ>&b), &f<size> &bblocks.";
    faction_claim_location_claimed = "&eLand at this &clocation &eis already &cclaimed&e.";
    faction_claim_spawn_too_close = "&eYou must be &c<amount> blocks &efrom spawn to claim land!";
    faction_claim_inventory_full = "&eThere is no place for &cclaiming wand &ein your inventory!";
    faction_claim_only_overworld = "&eYou can only claim in &coverworld&e.";
    faction_claim_buffer = "&eMinimal claim buffer size is &c<buffer> blocks&e.";
    faction_claim_warzone = "&eYou cannot claim land that belongs to &cWarzone&e!";
    faction_claim_claimed = "&c<player> &ehas claimed land between: &b<location1> &eand &b<location2>&e.";
    faction_unclaim_no_claims = "&eYour &cfaction &ehas no claims.";

    faction_deposit_deposited = "&c<player> &edeposited &b$<amount> &einto faction's balance.";
    faction_deposit_zero = "&eYou cannot deposit &c$0&e!";
    faction_deposit_not_enough = "&eYou only have &c$<amount>&e!";
    faction_withdraw_withdrawn = "&c<player> &ewithdrawn &a$&c<amount> &efrom faction's balance.";
    faction_withdraw_not_enough = "&eYour faction only has &a$&c<amount>&e!";

    faction_ally_added = "&eYour faction is now in alliance with &c<faction>&e.";
    faction_ally_removed = "&eYou are no longer allied with &c<faction>&e.";
    faction_ally_already = "&eYou are already ally with &c<faction>&e.";
    faction_ally_limit = "&eYour &cfaction &ealready reached maximum &callies &elimit.";
    faction_ally_request_sent = "&eFaction &c<faction> &ewere informed that you wish to be &callies&e.";
    faction_ally_request_received = "&eFaction &c<faction> &ehas sent you alliance request.";
    faction_leader_left = "&eYour faction leader left. Leadership transferred to &c<player>&e.";

    faction_chat_format = "&a(Faction) <player>&7: &e<message>";
    ally_chat_format = "&d(Ally) <player>&7: &e<message>";

    faction_map_no_nearby = "&eThere are no &cnearby claims &ein radius of &b<radius> &eblocks.";
    faction_map_owner_message = "<faction> &eowns land marked with &b<material>&e.";
    faction_map_disabled = "&eAll claim pillars are now &chidden&e.";

    faction_cannot_join_frozen = "&eYou cannot join a faction while frozen.";    faction_cannot_leave_frozen = "&eYou cannot leave your faction while frozen.";
    faction_cannot_disband_frozen = "&eYou cannot disband your faction while frozen.";
    faction_cannot_unclaim_frozen = "&eYou cannot unclaim while frozen.";

    // Usage / invite (exact port of language.yml)
    usage_invite = "Usage: /f invite <player>";
    usage_join = "&cUsage: /f join <faction>";
    usage_create = "&cUsage: /f create <name>";
    usage_kick = "&cUsage: /f kick <player>";
    usage_promote = "&cUsage: /f promote <player>";
    usage_demote = "&cUsage: /f demote <player>";
    usage_leader = "&cUsage: /f leader <player>";
    usage_rename = "&cUsage: /f rename <name>";
    usage_deposit = "&cUsage: /f deposit <amount>";
    usage_withdraw = "&cUsage: /f withdraw <amount>";
    usage_ally = "&cUsage: /f ally <faction>";
    usage_unally = "&cUsage: /f unally <faction>";
    faction_invite_others = "&ePlayer &c<player> &ehas been invited to faction.";
    safezone_deny_enter = "&eYou cannot enter &asafezone &ewhile &ccombat tagged&e!";

    // Territory
    territory_own = "&a<name>";
    territory_ally = "&d<name>";
    territory_enemy = "&c<name>";
    entering_territory = "&eEntering <faction> &e(<deathban>&e)";
    leaving_territory = "&eLeaving <faction> &e(<deathban>&e)";

    map_header = "&eFaction claim map";

    // ---- Timers ----
    combat_tagged = "&cYou are now combat tagged for &e<time>&c.";
    combat_tag_expired = "&aYou are no longer combat tagged.";
    combat_tag_logout_denied = "&cYou cannot log out while combat tagged.";
    enderpearl_cooldown = "&cYou cannot use enderpearls for another &e<time>&c.";
    gapple_cooldown = "&cYou cannot use golden apples for another &e<time>&c.";
    pvp_protection_active = "&aYou have PvP protection for &e<time>&a.";
    pvp_protection_expired = "&eYour &cpvp protection &ehas expired!";
    pvp_protection_denied = "&cYou cannot do that while protected.";

    // ---- PvP protection command (port of `PVP_COMMAND`) ----
    pvp_command_disabled = "&eYou have disabled your &cpvp protection&e!";
    pvp_command_not_active = "&eYour &cpvp protection &eis not active!";
    pvp_command_time = "&eYour &cpvp protection &etime is: &c<time>";
    pvp_command_usage_header = "&7&m                                                           ";
    pvp_command_usage_enable = " &c/pvp enable &f- &7Enable PvP for yourself";
    pvp_command_usage_time = " &c/pvp time &f- &7Check time left on your timer";
    pvp_command_usage_footer = "&7&m                                                           ";

    // ---- PvP protection restrictions (port of `PVP_PROTECTION`) ----
    pvp_protection_deny_teleport = "&eYou cannot teleport inside claims while your &cpvp protection &eis active!";
    pvp_protection_deny_attacker = "&eYou cannot attack &c<name> &ewhile their &cpvp protection &eis active!";
    pvp_protection_deny_victim = "&eYou cannot attack other players while your &cpvp protection &eis active!";
    pvp_protection_teleport_due_to_claim = "&eYou have been &cteleported &eoutside claimed land!";

    // ---- SOTW (port of `SOTW`) ----
    sotw_started = "&8[&a!&8] &aSOTW &ehas been started at &b<time>&e. &e&lGood Luck! &8[&a!&8]";
    sotw_ended = "&8[&c!&8] &aSOTW &ehas ended. &eWatch your back! &8[&c!&8]";
    sotw_not_running = "&cSotw timer &eis currently not running!";
    sotw_already_running = "&eThere is &csotw timer &ealready running!";
    sotw_enabled = "&eYou have &cdisabled &eyour &csotw &edamage protection!";
    sotw_already_enabled = "&eYou have already &cdisabled &eyour &csotw &edamage protection!";
    sotw_void_fix = "&eDue to active &csotw &eyou have been teleported to &cend spawn&e!";
    sotw_time_status = "&eRemaining &cSOTW &etimer time is: &c<time>";
    sotw_hide_players_enabled = "&eYou have &aenabled &esotw &chide players &efeature.";
    sotw_hide_players_disabled = "&eYou have &cdisabled &esotw &chide players &efeature.";
    sotw_admin_usage_header = "&7&m                                                           ";
    sotw_admin_usage_title = " &6&lSotw management";
    sotw_admin_usage_start = "  &e/sotw start [time]";
    sotw_admin_usage_stop = "  &e/sotw stop";
    sotw_admin_usage_hideplayers = "  &e/sotw hideplayers";
    sotw_admin_usage_footer = "&7&m                                                           ";
    sotw_player_usage_header = "&7&m                                                           ";
    sotw_player_usage_enable = " &c/sotw enable &f- &7Disable SOTW protection";
    sotw_player_usage_time = " &c/sotw time &f- &7Check SOTW time left";
    sotw_player_usage_footer = "&7&m                                                           ";

    // ---- Custom timers (port of `CUSTOM_TIMER_COMMAND`) ----
    custom_timer_usage_header = "&7&m                                                                ";
    custom_timer_usage_title = " &6&lCustom timer management";
    custom_timer_usage_start = "  &e/customtimer start <name> <placeholder> <time>";
    custom_timer_usage_cancel = "  &e/customtimer cancel <name>";
    custom_timer_usage_list = "  &e/customtimer list";
    custom_timer_usage_footer = "&7&m                                                                ";
    custom_timer_list_header = "&7&m                                                           ";
    custom_timer_list_title = " &6&lActive custom timers";
    custom_timer_list_footer = "&7&m                                                           ";
    custom_timer_list_format = " &f<id>. &7<timerName> &e- <placeholder>";
    custom_timer_not_running = "&eTimer with name &c<name> &eis not running!";
    custom_timer_started = "<timerName> &etimer has been started by &a<player>&e.";
    custom_timer_stopped = "<timerName> &etimer has been stopped by &c<player>&e.";
    home_warmup = "&eTeleporting to your faction home in &c<time>&e.";
    home_warmup_cancelled = "&cTeleport cancelled.";
    stuck_warmup = "&eYou will be teleported to &csafe location &ein &b<time>&e.";
    stuck_cancelled = "&cStuck teleport &ehas been cancelled because you moved more than &c5 blocks&e!";
    logout_started = "&eLogging out in &c<time>&e. Do not move.";
    logout_cancelled = "&cLogout cancelled.";

    // ---- Deathban / lives ----
    deathban_message = "&cYou are deathbanned for &e<time>&c.";
    deathban_lives_used = "&eA life was consumed. Lives left: &c<lives>&e.";
    deathban_revived = "&aYou were revived by &e<sender>&a.";
    deathban_not_banned = "&eThat player is not deathbanned.";
    deathban_revive_sender = "&eYou revived &c<player>&e.";
    lives_message = "&eLives: &c<lives>";
    lives_added = "&eYou gave &c<amount> &elives to &c<player>&e.";
    lives_set = "&eLives set to &c<amount> &efor &c<player>&e.";

    // ---- Vanish / staff ----
    vanish_enabled = "&aYou are now vanished.";
    vanish_disabled = "&cYou are no longer vanished.";
    staffmode_enabled = "&aYou are now in staff mode.";
    staffmode_disabled = "&cYou are no longer in staff mode.";
    admin_no_permission = "&cYou do not have permission.";

    // ---- Admin factions ----
    faction_admin_value_changed = "&eChanged &c<field> &eto &c<value> &efor faction &c<faction>&e.";
    faction_admin_force_kicked = "&eYou force-kicked &c<player> &efrom their faction.";

    // ---- Holograms ----
    hologram_created = "&aHologram &e<name> &acreated.";
    hologram_deleted = "&cHologram &e<name> &cdeleted.";
    hologram_not_found = "&cHologram &e<name> &cdoes not exist.";
    hologram_line_added = "&aLine added to hologram &e<name>&a.";
    hologram_line_removed = "&cLine removed from hologram &e<name>&c.";
}

impl Lang {
    /// Formats a duration for scoreboard displays.
    pub fn duration(millis: i64) -> String {
        crate::utils::format_duration(millis)
    }
}
