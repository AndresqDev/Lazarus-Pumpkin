//! `/f` faction command tree — port of `factions.commands.FactionsCommandExecutor`
//! and the player subcommands.
//!
//! The original relied on a WorldEdit-style selection wand for claiming. Because the
//! port has no selection system yet, `/f claim` claims the single chunk the player
//! stands in (the classic "chunk claim" mode). This is documented in `PORTING.md`.

use pumpkin_plugin_api::command::{
    ArgumentType, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType,
};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Player, Server};

use super::{command, double_arg, int_arg, register_op_permission, register_public_permission, reply, string_arg};
use crate::config::Config;
use crate::core;
use crate::factions::{member, Claim, ClaimManager, Faction, FactionsManager, Role};
use crate::glass::GlassType;
use crate::lang::Lang;
use crate::state::{state, AppState, ChunkState};
use crate::timers::{PendingAction, TimerType};
use crate::util;

#[derive(Clone, Copy)]
enum Action {
    Create,
    Disband,
    Invite,
    Join,
    Leave,
    Kick,
    Promote,
    Demote,
    Leader,
    Rename,
    Chat,
    Claim,
    Unclaim,
    UnclaimAll,
    Map,
    Territory,
    TerritoryCoords,
    Help,
    SetHome,
    Home,
    Stuck,
    Open,
    ShowSelf,
    ShowNamed,
    List,
    TopDefault,
    TopMetric,
    Deposit,
    Withdraw,
    Ally,
    Unally,
    AdminSetDtr,
    AdminAddDtr,
    AdminRemoveDtr,
    AdminRemove,
    AdminClearClaims,
    AdminSetKills,
    AdminForcePromote,
    AdminForceDemote,
    AdminForceJoin,
    AdminForceRename,
    AdminSystemCreate,
    AdminSystemList,
    AdminSystemRename,
    AdminSafezone,
    AdminEnderpearls,
    AdminAbilities,
    AdminColor,
    AdminDeathban,
    AdminClaimFor,
    AdminRemoveClaim,
    AdminChatSpy,
    AdminTpHere,
    AdminSetPoints,
    AdminSetBalance,
    AdminSetLives,
    AdminForceKick,
    AdminForceLeader,
    AdminFreeze,
    AdminThaw,
    AdminSave,
    HelpPage,
}

struct FactionExecutor(Action);

impl CommandHandler for FactionExecutor {
    fn handle(&self, sender: CommandSender, server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        if let Action::Help = self.0 {
            help(&sender, 1);
            return Ok(1);
        }
        if let Action::HelpPage = self.0 {
            let page = int_arg(&args, "page").unwrap_or(1) as usize;
            help(&sender, page);
            return Ok(1);
        }
        run(self.0, &sender, &server, &args);
        Ok(1)
    }
}

pub fn register(context: &Context) -> Result<(), String> {
    register_public_permission(context, "lazarus:command.faction", "Use the faction commands")?;
    register_op_permission(context, "lazarus:command.faction.admin", "Use the faction admin commands")?;
    for node in [
        "lazarus:factions.dtr",
        "lazarus:factions.setpoints",
        "lazarus:factions.setbalance",
        "lazarus:factions.setlives",
        "lazarus:factions.forcekick",
        "lazarus:factions.forceleader",
        "lazarus:factions.setfreeze",
        "lazarus:factions.thaw",
        "lazarus:factions.save",
        "lazarus:factions.remove",
        "lazarus:factions.clearclaims",
        "lazarus:factions.setkills",
        "lazarus:factions.forcepromote",
        "lazarus:factions.forcedemote",
        "lazarus:factions.forcejoin",
        "lazarus:factions.forcerename",
        "lazarus:factions.systemcreate",
        "lazarus:factions.systemlist",
        "lazarus:factions.systemrename",
        "lazarus:factions.safezone",
        "lazarus:factions.enderpearls",
        "lazarus:factions.abilities",
        "lazarus:factions.color",
        "lazarus:factions.deathban",
        "lazarus:factions.claimfor",
        "lazarus:factions.removeclaim",
        "lazarus:factions.chatspy",
        "lazarus:factions.tphere",
    ] {
        register_op_permission(context, node, "Lazarus faction admin command")?;
    }

    let str_node = |name: &str| CommandNode::argument(name, &ArgumentType::String(StringType::SingleWord));
    let int_node = |name: &str| CommandNode::argument(name, &ArgumentType::Integer((None, None)));

    let root = command(&["f", "faction", "fac", "factions"], "Faction commands")
        .execute(FactionExecutor(Action::Help))
        .then(
            CommandNode::literal("help")
                .execute(FactionExecutor(Action::Help))
                .then(int_node("page").execute(FactionExecutor(Action::HelpPage))),
        )
        .then(CommandNode::literal("create").then(str_node("name").execute(FactionExecutor(Action::Create))))
        .then(CommandNode::literal("new").then(str_node("name").execute(FactionExecutor(Action::Create))))
        .then(CommandNode::literal("disband").execute(FactionExecutor(Action::Disband)))
        .then(CommandNode::literal("invite").then(str_node("player").execute(FactionExecutor(Action::Invite))))
        .then(CommandNode::literal("inv").then(str_node("player").execute(FactionExecutor(Action::Invite))))
        .then(CommandNode::literal("join").then(str_node("name").execute(FactionExecutor(Action::Join))))
        .then(CommandNode::literal("accept").then(str_node("name").execute(FactionExecutor(Action::Join))))
        .then(CommandNode::literal("leave").execute(FactionExecutor(Action::Leave)))
        .then(CommandNode::literal("kick").then(str_node("player").execute(FactionExecutor(Action::Kick))))
        .then(CommandNode::literal("promote").then(str_node("player").execute(FactionExecutor(Action::Promote))))
        .then(CommandNode::literal("demote").then(str_node("player").execute(FactionExecutor(Action::Demote))))
        .then(CommandNode::literal("leader").then(str_node("player").execute(FactionExecutor(Action::Leader))))
        .then(CommandNode::literal("rename").then(str_node("name").execute(FactionExecutor(Action::Rename))))
        .then(CommandNode::literal("chat").execute(FactionExecutor(Action::Chat)))
        .then(CommandNode::literal("c").execute(FactionExecutor(Action::Chat)))
        .then(CommandNode::literal("claim").execute(FactionExecutor(Action::Claim)))
        .then(CommandNode::literal("unclaim").execute(FactionExecutor(Action::Unclaim)))
        .then(CommandNode::literal("unclaimall").execute(FactionExecutor(Action::UnclaimAll)))
        .then(CommandNode::literal("map").execute(FactionExecutor(Action::Map)))
        .then(CommandNode::literal("territory").execute(FactionExecutor(Action::Territory)).then(
            int_node("x").then(int_node("z").execute(FactionExecutor(Action::TerritoryCoords)))
        ))
        .then(CommandNode::literal("sethome").execute(FactionExecutor(Action::SetHome)))
        .then(CommandNode::literal("home").execute(FactionExecutor(Action::Home)))
        .then(CommandNode::literal("stuck").execute(FactionExecutor(Action::Stuck)))
        .then(CommandNode::literal("open").execute(FactionExecutor(Action::Open)))
        .then(
            CommandNode::literal("show")
                .execute(FactionExecutor(Action::ShowSelf))
                .then(str_node("faction").execute(FactionExecutor(Action::ShowNamed))),
        )
        .then(CommandNode::literal("i").execute(FactionExecutor(Action::ShowSelf)))
        .then(CommandNode::literal("info").execute(FactionExecutor(Action::ShowSelf)))
        .then(CommandNode::literal("who").execute(FactionExecutor(Action::ShowSelf)))
        .then(CommandNode::literal("list").execute(FactionExecutor(Action::List)))
        .then(
            CommandNode::literal("top")
                .execute(FactionExecutor(Action::TopDefault))
                .then(str_node("metric").execute(FactionExecutor(Action::TopMetric))),
        )
        .then(CommandNode::literal("deposit").then(str_node("amount").execute(FactionExecutor(Action::Deposit))))
        .then(CommandNode::literal("d").then(str_node("amount").execute(FactionExecutor(Action::Deposit))))
        .then(CommandNode::literal("withdraw").then(str_node("amount").execute(FactionExecutor(Action::Withdraw))))
        .then(CommandNode::literal("w").then(str_node("amount").execute(FactionExecutor(Action::Withdraw))))
        .then(CommandNode::literal("ally").then(str_node("faction").execute(FactionExecutor(Action::Ally))))
        .then(CommandNode::literal("unally").then(str_node("faction").execute(FactionExecutor(Action::Unally))))
        // ---- Admin subcommands ----
        .then(
            CommandNode::literal("setdtr").then(
                str_node("faction").then(
                    CommandNode::argument("value", &ArgumentType::Double((None, None)))
                        .execute(FactionExecutor(Action::AdminSetDtr)),
                ),
            ),
        )
        .then(
            CommandNode::literal("adddtr").then(
                str_node("faction").then(
                    CommandNode::argument("value", &ArgumentType::Double((None, None)))
                        .execute(FactionExecutor(Action::AdminAddDtr)),
                ),
            ),
        )
        .then(
            CommandNode::literal("removedtr").then(
                str_node("faction").then(
                    CommandNode::argument("value", &ArgumentType::Double((None, None)))
                        .execute(FactionExecutor(Action::AdminRemoveDtr)),
                ),
            ),
        )
        .then(
            CommandNode::literal("setpoints").then(
                str_node("faction").then(int_node("value").execute(FactionExecutor(Action::AdminSetPoints))),
            ),
        )
        .then(
            CommandNode::literal("setbalance").then(
                str_node("faction").then(int_node("value").execute(FactionExecutor(Action::AdminSetBalance))),
            ),
        )
        .then(
            CommandNode::literal("setlives").then(
                str_node("faction").then(int_node("value").execute(FactionExecutor(Action::AdminSetLives))),
            ),
        )
        .then(CommandNode::literal("forcekick").then(str_node("player").execute(FactionExecutor(Action::AdminForceKick))))
        .then(CommandNode::literal("forceleader").then(str_node("player").execute(FactionExecutor(Action::AdminForceLeader))))
        .then(CommandNode::literal("freeze").then(str_node("faction").execute(FactionExecutor(Action::AdminFreeze))))
        .then(CommandNode::literal("thaw").then(str_node("faction").execute(FactionExecutor(Action::AdminThaw))))
        .then(CommandNode::literal("save").execute(FactionExecutor(Action::AdminSave)))
        .then(CommandNode::literal("remove").then(str_node("faction").execute(FactionExecutor(Action::AdminRemove))))
        .then(CommandNode::literal("clearclaims").then(str_node("faction").execute(FactionExecutor(Action::AdminClearClaims))))
        .then(
            CommandNode::literal("setkills").then(
                str_node("faction").then(int_node("amount").execute(FactionExecutor(Action::AdminSetKills))),
            ),
        )
        .then(CommandNode::literal("forcepromote").then(str_node("player").execute(FactionExecutor(Action::AdminForcePromote))))
        .then(CommandNode::literal("forcedemote").then(str_node("player").execute(FactionExecutor(Action::AdminForceDemote))))
        .then(CommandNode::literal("forcejoin").then(str_node("faction").execute(FactionExecutor(Action::AdminForceJoin))))
        .then(CommandNode::literal("forcerename").then(str_node("faction").then(str_node("name").execute(FactionExecutor(Action::AdminForceRename)))))
        .then(CommandNode::literal("systemcreate").then(str_node("name").execute(FactionExecutor(Action::AdminSystemCreate))))
        .then(CommandNode::literal("systemlist").execute(FactionExecutor(Action::AdminSystemList)))
        .then(CommandNode::literal("systemrename").then(str_node("faction").then(str_node("name").execute(FactionExecutor(Action::AdminSystemRename)))))
        .then(CommandNode::literal("safezone").then(str_node("faction").execute(FactionExecutor(Action::AdminSafezone))))
        .then(CommandNode::literal("enderpearls").then(str_node("faction").execute(FactionExecutor(Action::AdminEnderpearls))))
        .then(CommandNode::literal("abilities").then(str_node("faction").execute(FactionExecutor(Action::AdminAbilities))))
        .then(CommandNode::literal("color").then(str_node("faction").then(str_node("color").execute(FactionExecutor(Action::AdminColor)))))
        .then(CommandNode::literal("deathban").then(str_node("faction").execute(FactionExecutor(Action::AdminDeathban))))
        .then(CommandNode::literal("claimfor").then(str_node("faction").execute(FactionExecutor(Action::AdminClaimFor))))
        .then(CommandNode::literal("removeclaim").execute(FactionExecutor(Action::AdminRemoveClaim)))
        .then(CommandNode::literal("chatspy").execute(FactionExecutor(Action::AdminChatSpy)))
        .then(CommandNode::literal("tphere").then(str_node("faction").execute(FactionExecutor(Action::AdminTpHere))));

    context.register_command(root, "lazarus:command.faction");
    Ok(())
}

fn run(action: Action, sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let Some(player) = sender.as_player() else {
        reply(sender, "&cThis command is for players only.");
        return;
    };
    let uuid = core::uuid_of(&player);
    let name = core::name_of(&player);

    match action {
        Action::Create => create(sender, &player, &uuid, &name, args),
        Action::Disband => disband(sender, server, &uuid),
        Action::Invite => invite(sender, server, &uuid, args),
        Action::Join => join(sender, server, &player, &uuid, &name, args),
        Action::Leave => leave(sender, server, &player, &uuid, &name),
        Action::Kick => kick(sender, server, &uuid, args),
        Action::Promote => change_role(sender, server, &uuid, args, true),
        Action::Demote => change_role(sender, server, &uuid, args, false),
        Action::Leader => transfer_leader(sender, server, &uuid, args),
        Action::Rename => rename(sender, &uuid, args),
        Action::Chat => toggle_chat(sender, &uuid),
        Action::Claim => claim(sender, &player, &uuid),
        Action::Unclaim => unclaim(sender, &player, &uuid),
        Action::UnclaimAll => unclaim_all(sender, &uuid),
        Action::Map => map(sender, &player, &uuid),
        Action::Territory => territory_info(sender, &player, &uuid, None),
        Action::TerritoryCoords => {
            let x = int_arg(args, "x").unwrap_or(0) as i32;
            let z = int_arg(args, "z").unwrap_or(0) as i32;
            territory_info(sender, &player, &uuid, Some((x, z)))
        }
        Action::Help => {}
        Action::HelpPage => {}
        Action::SetHome => set_home(sender, &player, &uuid),
        Action::Home => home(sender, &player, &uuid),
        Action::Stuck => stuck(sender, &player, &uuid),
        Action::Open => toggle_open(sender, server, &uuid),
        Action::ShowSelf => show(sender, server, &uuid, None),
        Action::ShowNamed => show(sender, server, &uuid, string_arg(args, "faction")),
        Action::List => list(sender),
        Action::TopDefault => top(sender, "points"),
        Action::TopMetric => top(sender, &string_arg(args, "metric").unwrap_or_else(|| "points".into())),
        Action::Deposit => transfer_balance(sender, &uuid, args, true),
        Action::Withdraw => transfer_balance(sender, &uuid, args, false),
        Action::Ally => ally(sender, server, &uuid, args, true),
        Action::Unally => ally(sender, server, &uuid, args, false),
        Action::AdminSetDtr => admin_set_dtr(sender, server, args),
        Action::AdminAddDtr => admin_change_dtr(sender, server, args, true),
        Action::AdminRemoveDtr => admin_change_dtr(sender, server, args, false),
        Action::AdminRemove => admin_remove(sender, server, args),
        Action::AdminClearClaims => admin_clear_claims(sender, server, args),
        Action::AdminSetKills => admin_set_kills(sender, server, args),
        Action::AdminForcePromote => admin_force_role(sender, server, args, true),
        Action::AdminForceDemote => admin_force_role(sender, server, args, false),
        Action::AdminForceJoin => admin_force_join(sender, server, args),
        Action::AdminForceRename => admin_force_rename(sender, server, args),
        Action::AdminSystemCreate => admin_system_create(sender, server, args),
        Action::AdminSystemList => admin_system_list(sender),
        Action::AdminSystemRename => admin_system_rename(sender, server, args),
        Action::AdminSafezone => admin_toggle_system_flag(sender, server, args, SystemFlag::Safezone),
        Action::AdminEnderpearls => admin_toggle_system_flag(sender, server, args, SystemFlag::Enderpearls),
        Action::AdminAbilities => admin_toggle_system_flag(sender, server, args, SystemFlag::Abilities),
        Action::AdminColor => admin_color(sender, server, args),
        Action::AdminDeathban => admin_deathban(sender, server, args),
        Action::AdminClaimFor => admin_claim_for(sender, server, args),
        Action::AdminRemoveClaim => admin_remove_claim(sender, server),
        Action::AdminChatSpy => admin_chat_spy(sender, server),
        Action::AdminTpHere => admin_tp_here(sender, server, args),
        Action::AdminSetPoints => admin_set_int(sender, server, args, AdminField::Points),
        Action::AdminSetBalance => admin_set_int(sender, server, args, AdminField::Balance),
        Action::AdminSetLives => admin_set_int(sender, server, args, AdminField::Lives),
        Action::AdminForceKick => admin_force_kick(sender, server, args),
        Action::AdminForceLeader => admin_force_leader(sender, server, args),
        Action::AdminFreeze => admin_freeze(sender, server, args, true),
        Action::AdminThaw => admin_freeze(sender, server, args, false),
        Action::AdminSave => admin_save(sender, server),
    }
}

// ---------------------------------------------------------------------------
// Individual subcommands
// ---------------------------------------------------------------------------

fn create(sender: &CommandSender, _player: &Player, uuid: &str, name: &str, args: &ConsumedArgs) {
    let Some(faction_name) = string_arg(args, "name") else {
        reply(sender, &state().read().unwrap().lang.usage_create);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let config = guard.config.clone();
        let lang = guard.lang.clone();

        if guard.factions.by_member(uuid).is_some() {
            reply(sender, &lang.faction_already_in_self);
            return;
        }

        if faction_name.chars().count() < config.factions.name_min_length {
            reply(sender, util::replace(&lang.faction_name_too_short, "<min>", &config.factions.name_min_length.to_string()));
            return;
        }
        if faction_name.chars().count() > config.factions.name_max_length {
            reply(sender, util::replace(&lang.faction_name_too_long, "<max>", &config.factions.name_max_length.to_string()));
            return;
        }
        if config.factions.disallowed_names.iter().any(|n| n.eq_ignore_ascii_case(&faction_name)) {
            reply(sender, &lang.faction_blocked_name);
            return;
        }
        if guard.factions.name_taken(&faction_name) {
            reply(sender, util::replace(&lang.faction_already_exists, "<name>", &faction_name));
            return;
        }

        let id = guard.factions.create(faction_name.clone(), &config);
        guard.factions.add_member(&id, member(uuid.to_string(), name.to_string(), Role::Leader));

        messages = util::replace(&lang.faction_created, "<name>", &faction_name)
            .replace("<player>", name);
    }

    reply(sender, messages);
}

fn disband(sender: &CommandSender, server: &Server, uuid: &str) {
    let messages: Option<String>;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();
        let config = guard.config.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();

        if !is_leader(&faction, uuid) {
            reply(sender, &lang.faction_leader_only);
            return;
        }
        if config.factions.disband_while_frozen && guard.timers.is_frozen(&faction_id) {
            reply(sender, &lang.faction_cannot_disband_frozen);
            return;
        }

        guard.claims.remove_all(&faction_id);
        guard.factions.disband(&faction_id);

        let message = util::replace(
            &util::replace(&lang.faction_disbanded, "<name>", &faction.name),
            "<player>",
            &sender.get_name(),
        );
        // `PlayerFaction#sendMessage` reaches every member, the commanding leader
        // included, so there is no separate reply here (that caused a double send).
        for member in faction.members.values() {
            if let Some(uuid) = pumpkin_plugin_api::uuid::parse(&member.uuid)
                && let Some(player) = server.get_player_by_uuid(uuid)
            {
                core::send(&player, &message);
            }
        }
        messages = None;
    }
    if let Some(message) = messages {
        reply(sender, message);
    }
}

fn invite(sender: &CommandSender, server: &Server, uuid: &str, args: &ConsumedArgs) {
    let Some(target_name) = string_arg(args, "player") else {
        reply(sender, &state().read().unwrap().lang.usage_invite);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();
        let config = guard.config.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };

        let Some(target) = server.get_player_by_name(&target_name) else {
            reply(sender, util::replace(&lang.player_not_online, "<player>", &target_name));
            return;
        };
        let target_uuid = core::uuid_of(&target);

        if guard.factions.by_member(&target_uuid).is_some() {
            reply(sender, util::replace(&lang.faction_already_in_others, "<player>", &target_name));
            return;
        }

        let Some(faction) = guard.factions.get(&faction_id) else {
            return;
        };

        if faction.invitations.contains(&target_uuid) {
            reply(sender, util::replace(&lang.faction_invite_already, "<player>", &target_name));
            return;
        }
        if faction.members.len() >= config.factions.faction_limit {
            reply(sender, &lang.faction_full);
            return;
        }

        let faction_name = faction.name.clone();
        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            faction.invitations.push(target_uuid.clone());
        }

        let to_target = util::replace(&lang.faction_invited, "<name>", &faction_name);
        let to_sender = util::replace(&lang.faction_invite_others, "<player>", &target_name);

        core::send(&target, &to_target);
        messages = to_sender;
    }
    reply(sender, messages);
}

fn join(sender: &CommandSender, server: &Server, player: &Player, uuid: &str, name: &str, args: &ConsumedArgs) {
    let Some(faction_name) = string_arg(args, "name") else {
        reply(sender, &state().read().unwrap().lang.usage_join);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();
        let config = guard.config.clone();

        if guard.factions.by_member(uuid).is_some() {
            reply(sender, &lang.faction_already_in_self);
            return;
        }

        let Some(faction_id) = guard.factions.by_name(&faction_name).filter(|f| f.is_player()).map(|f| f.id.clone()) else {
            reply(sender, util::replace(&lang.faction_doesnt_exist, "<argument>", &faction_name));
            return;
        };

        let invited = guard.factions.get(&faction_id).is_some_and(|f| f.invitations.contains(&uuid.to_string()));
        let open = guard.factions.get(&faction_id).is_some_and(|f| f.open);
        if !invited && !open {
            reply(sender, &lang.faction_no_invitation);
            return;
        }

        if config.factions.join_while_frozen && guard.timers.is_frozen(&faction_id) {
            reply(sender, &lang.faction_cannot_join_frozen);
            return;
        }

        if let Some(faction) = guard.factions.get(&faction_id) {
            if faction.members.len() >= config.factions.faction_limit {
                reply(sender, &lang.faction_full);
                return;
            }
        }

        guard.factions.add_member(&faction_id, member(uuid.to_string(), name.to_string(), Role::Member));
        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            faction.invitations.retain(|id| id != uuid);
        }
        guard
            .chunk_cache
            .insert(uuid.to_string(), ChunkState::default());

        let faction = guard.factions.get(&faction_id).cloned().unwrap();
        let broadcast = util::replace(&lang.faction_member_joined, "<player>", name);
        for member in faction.members.values() {
            if member.uuid == uuid {
                continue;
            }
            if let Some(id) = pumpkin_plugin_api::uuid::parse(&member.uuid)
                && let Some(online) = server.get_player_by_uuid(id)
            {
                core::send(&online, &broadcast);
            }
        }
        messages = util::replace(&lang.faction_joined, "<player>", name);
        let _ = player;
    }
    reply(sender, messages);
}

fn leave(sender: &CommandSender, server: &Server, player: &Player, uuid: &str, name: &str) {
    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();
        let config = guard.config.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();

        if is_leader(&faction, uuid) && faction.members.len() > 1 {
            reply(sender, &lang.faction_leader_only);
            return;
        }
        if !config.factions.leave_while_frozen && guard.timers.is_frozen(&faction_id) {
            reply(sender, &lang.faction_cannot_leave_frozen);
            return;
        }

        guard.factions.remove_member(&faction_id, uuid);
        if faction.members.len() <= 1 {
            guard.claims.remove_all(&faction_id);
            guard.factions.disband(&faction_id);
        }

        let broadcast = util::replace(&lang.faction_member_left, "<player>", name);
        for member in faction.members.values() {
            if member.uuid == uuid {
                continue;
            }
            if let Some(id) = pumpkin_plugin_api::uuid::parse(&member.uuid)
                && let Some(online) = server.get_player_by_uuid(id)
            {
                core::send(&online, &broadcast);
            }
        }
        messages = util::replace(&lang.faction_left, "<name>", &faction.name);
        let _ = player;
    }
    reply(sender, messages);
}

fn kick(sender: &CommandSender, server: &Server, uuid: &str, args: &ConsumedArgs) {
    let Some(target_name) = string_arg(args, "player") else {
        reply(sender, &state().read().unwrap().lang.usage_kick);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();

        let Some(sender_role) = faction.member(uuid).map(|m| m.role) else {
            return;
        };
        if !sender_role.is_at_least(Role::Captain) {
            reply(sender, &lang.faction_leader_only);
            return;
        }

        let Some(target) = server.get_player_by_name(&target_name) else {
            reply(sender, util::replace(&lang.player_not_online, "<player>", &target_name));
            return;
        };
        let target_uuid = core::uuid_of(&target);

        let Some(target_role) = faction.member(&target_uuid).map(|m| m.role) else {
            reply(sender, util::replace(&lang.faction_not_in_others, "<player>", &target_name));
            return;
        };
        if target_role.is_at_least(sender_role) {
            reply(sender, &lang.faction_leader_only);
            return;
        }
        if target_uuid == uuid {
            return;
        }

        guard.factions.remove_member(&faction_id, &target_uuid);
        core::send(&target, &util::replace(&lang.faction_you_were_kicked, "<name>", &faction.name));

        messages = util::replace(&lang.faction_kicked, "<player>", &target_name);
    }
    reply(sender, messages);
}

fn change_role(sender: &CommandSender, server: &Server, uuid: &str, args: &ConsumedArgs, promote: bool) {
    let Some(target_name) = string_arg(args, "player") else {
        let usage = if promote {
            state().read().unwrap().lang.usage_promote.clone()
        } else {
            state().read().unwrap().lang.usage_demote.clone()
        };
        reply(sender, usage);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();

        let Some(sender_role) = faction.member(uuid).map(|m| m.role) else {
            return;
        };
        if !sender_role.is_at_least(Role::CoLeader) {
            reply(sender, &lang.faction_leader_only);
            return;
        }

        let Some(target) = server.get_player_by_name(&target_name) else {
            reply(sender, util::replace(&lang.player_not_online, "<player>", &target_name));
            return;
        };
        let target_uuid = core::uuid_of(&target);

        let Some(target_role) = faction.member(&target_uuid).map(|m| m.role) else {
            reply(sender, util::replace(&lang.faction_not_in_others, "<player>", &target_name));
            return;
        };

        let new_role = if promote { target_role.promote() } else { target_role.demote() };
        let Some(new_role) = new_role else {
            return;
        };

        if let Some(faction) = guard.factions.get_mut(&faction_id)
            && let Some(member) = faction.member_mut(&target_uuid)
        {
            member.role = new_role;
        }

        let template = if promote { &lang.faction_promoted } else { &lang.faction_demoted };
        messages = util::replace(
            &util::replace(template, "<player>", &target_name),
            "<role>",
            new_role.name(),
        );
    }
    reply(sender, messages);
}

fn transfer_leader(sender: &CommandSender, server: &Server, uuid: &str, args: &ConsumedArgs) {
    let Some(target_name) = string_arg(args, "player") else {
        reply(sender, &state().read().unwrap().lang.usage_leader);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();
        if !is_leader(&faction, uuid) {
            reply(sender, &lang.faction_leader_only);
            return;
        }

        let Some(target) = server.get_player_by_name(&target_name) else {
            reply(sender, util::replace(&lang.player_not_online, "<player>", &target_name));
            return;
        };
        let target_uuid = core::uuid_of(&target);
        if !faction.members.contains_key(&target_uuid) {
            reply(sender, util::replace(&lang.faction_not_in_others, "<player>", &target_name));
            return;
        }

        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            if let Some(old_leader) = faction.member_mut(uuid) {
                old_leader.role = Role::CoLeader;
            }
            if let Some(new_leader) = faction.member_mut(&target_uuid) {
                new_leader.role = Role::Leader;
            }
        }

        messages = util::replace(
            &util::replace(&lang.faction_leader_changed, "<player>", &sender.get_name()),
            "<target>",
            &target_name,
        );
    }
    reply(sender, messages);
}

fn rename(sender: &CommandSender, uuid: &str, args: &ConsumedArgs) {
    let Some(new_name) = string_arg(args, "name") else {
        reply(sender, &state().read().unwrap().lang.usage_rename);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();
        let config = guard.config.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();
        if !is_leader(&faction, uuid) {
            reply(sender, &lang.faction_leader_only);
            return;
        }
        if guard.factions.name_taken(&new_name) {
            reply(sender, util::replace(&lang.faction_already_exists, "<name>", &new_name));
            return;
        }
        if new_name.chars().count() < config.factions.name_min_length
            || new_name.chars().count() > config.factions.name_max_length
        {
            reply(sender, &lang.faction_blocked_name);
            return;
        }
        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            faction.name = new_name.clone();
        }
        messages = util::replace(
            &util::replace(&lang.faction_renamed, "<name>", &faction.name),
            "<newName>",
            &new_name,
        );
    }
    reply(sender, messages);
}

fn toggle_chat(sender: &CommandSender, uuid: &str) {
    let mut guard = state().write().unwrap();
    let lang = guard.lang.clone();
    let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
        reply(sender, &lang.faction_not_in);
        return;
    };
    let mut new_type = crate::factions::ChatType::Public;
    if let Some(faction) = guard.factions.get_mut(&faction_id)
        && let Some(member) = faction.member_mut(uuid)
    {
        member.chat_type = match member.chat_type {
            crate::factions::ChatType::Public => crate::factions::ChatType::Faction,
            crate::factions::ChatType::Faction => crate::factions::ChatType::Ally,
            crate::factions::ChatType::Ally => crate::factions::ChatType::Public,
        };
        new_type = member.chat_type;
    }
    let status = match new_type {
        crate::factions::ChatType::Public => "public",
        crate::factions::ChatType::Faction => "faction",
        crate::factions::ChatType::Ally => "ally",
    };
    reply(sender, util::replace(&lang.faction_chat_toggled, "<status>", status));
}

fn claim(sender: &CommandSender, player: &Player, uuid: &str) {
    let error = {
        let guard = state().read().unwrap();
        validate_claim(player, uuid, &guard.factions, &guard.claims, &guard.config, &guard.lang)
    };

    if let Err(message) = error {
        reply(sender, message);
        return;
    }

    // The original always (re)starts the selection process, it never toggles off.
    state().write().unwrap().selection.start(player);
    let message = state().read().unwrap().lang.faction_claim_wand_received.clone();
    reply(sender, message);
}

fn validate_claim(
    player: &Player,
    uuid: &str,
    factions: &FactionsManager,
    claims: &ClaimManager,
    config: &Config,
    lang: &Lang,
) -> Result<(), String> {
    let Some(faction_id) = factions.by_member(uuid).map(|f| f.id.clone()) else {
        return Err(lang.faction_not_in.clone());
    };
    let faction = factions.get(&faction_id).unwrap();

    if faction
        .member(uuid)
        .map(|m| m.role)
        .is_none_or(|role| !role.is_at_least(Role::CoLeader))
    {
        return Err(lang.faction_leader_only.clone());
    }

    let max_claims = config
        .factions
        .max_claims
        .min(config.factions.claims_per_player * faction.members.len());
    if claims.claim_count(&faction_id) >= max_claims {
        return Err(lang.faction_claim_limit.clone());
    }

    if core::dimension_of(&core::world_name(player)) != core::Dimension::Overworld {
        return Err(lang.faction_claim_only_overworld.clone());
    }

    let location = core::location(player);
    let warzone = config.warzone_radius.overworld;
    if location.block_x().abs() <= warzone && location.block_z().abs() <= warzone {
        return Err(util::replace(
            &lang.faction_claim_spawn_too_close,
            "<amount>",
            &warzone.to_string(),
        ));
    }

    if (0..36u8).all(|slot| player.get_inventory_item(slot).is_some()) {
        return Err(lang.faction_claim_inventory_full.clone());
    }

    Ok(())
}

fn unclaim(sender: &CommandSender, player: &Player, uuid: &str) {
    let loc = core::location(player);
    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let owned = guard
            .claims
            .faction_at(&loc.world, loc.block_x(), loc.block_z())
            .map(|owner| owner == &faction_id)
            .unwrap_or(false);
        if !owned {
            reply(sender, &lang.faction_claim_not_owned);
            return;
        }
        guard
            .claims
            .remove_claim_at(&faction_id, &loc.world, loc.block_x(), loc.block_z());
        messages = util::replace(
            &util::replace(&lang.faction_claim_removed, "<player>", &sender.get_name()),
            "<location>",
            &format!("{}, {}", loc.block_x(), loc.block_z()),
        );
    }
    reply(sender, messages);
}

fn unclaim_all(sender: &CommandSender, uuid: &str) {
    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();
        if !is_leader(&faction, uuid) {
            reply(sender, &lang.faction_leader_only);
            return;
        }
        guard.claims.remove_all(&faction_id);
        messages = "&aAll faction claims removed.".to_string();
    }
    reply(sender, messages);
}

fn map(sender: &CommandSender, player: &Player, uuid: &str) {
    let world = core::world_name(player);
    let message;
    {
        let mut guard = state().write().unwrap();
        let AppState { glass, claims, config, .. } = &mut *guard;

        if glass.is_map_user(uuid) {
            glass.clear_type(player, GlassType::ClaimMap);
            glass.remove_map_user(uuid);
            message = "&eAll claim pillars are now &chidden&e.".to_string();
        } else {
            // Force a regeneration on the next map update.
            glass.set_map_user(uuid, &world, i32::MIN, i32::MIN);
            crate::glass::update_map(glass, player, claims, config);
            message = if glass.is_map_user(uuid) {
                "&aClaim map enabled.".to_string()
            } else {
                "&cThere are no claims nearby.".to_string()
            };
        }
    }
    reply(sender, message);
}

/// Diagnostic helper (not part of the original): prints the territory square with
/// the warzone/safezone bounds centered at 0,0.
fn territory_info(
    sender: &CommandSender,
    player: &Player,
    uuid: &str,
    coords: Option<(i32, i32)>,
) {
    let location = core::location(player);
    let world = location.world.clone();
    let (bx, bz) = coords.unwrap_or((location.block_x(), location.block_z()));

    let (war_radius, safe_radius) = {
        let guard = state().read().unwrap();
        let war = match core::dimension_of(&world) {
            core::Dimension::Overworld => guard.config.warzone_radius.overworld,
            core::Dimension::Nether => guard.config.warzone_radius.nether,
            core::Dimension::End => guard.config.warzone_radius.end,
        };
        (war, guard.config.spawn.safezone_radius)
    };

    let r = war_radius;
    let s = safe_radius;
    let dist = bx.abs().max(bz.abs());

    let territory = core::territory_at(&world, bx, bz);
    let viewer = core::faction_id_of(uuid);
    let display = core::territory_display(&territory, viewer.as_deref());

    let reason = if dist <= s {
        "Safezone (dist <= safezone)"
    } else if dist <= r {
        "Warzone (dist <= warzone)"
    } else {
        "Wilderness (dist > warzone)"
    };

    let sep = format!("&7&m{}", " ".repeat(80));
    reply(sender, &sep);
    reply(sender, "&6&lWarzone square (centro 0,0)");
    reply(sender, format!(" &eWorld: &f{world}"));
    reply(sender, format!(" &eSafezone: &fX[-{s}, {s}] &7Z[&f-{s}, {s}&7] &8(|x|,|z| <= {s})"));
    reply(sender, format!(" &eWarzone: &fX[-{r}, {r}] &7Z[&f-{r}, {r}&7] &8(|x|,|z| <= {r})"));
    reply(
        sender,
        format!(
            " &eCorners: &f(-{r},-{r}) (-{r},{r}) ({r},-{r}) ({r},{r})"
        ),
    );
    reply(sender, format!(" &eLado: &f{} &ebloques por eje (inclusivo)", r * 2 + 1));
    reply(sender, " &7Protección &finfinita en Y&7 (arriba/abajo).");
    reply(sender, &sep);

    reply(sender, format!(" &eQuery: &f{bx}, {bz}"));
    reply(sender, format!(" &eDist: &fmax(|x|,|z|) = {dist}"));
    reply(sender, format!(" &eCurrent: {display}"));
    reply(sender, format!(" &eReason: &f{reason}"));
    reply(sender, &sep);

    // Corner verification: every corner must be Warzone, and one block outside must
    // be Wilderness — proves the square on all four sides.
    reply(sender, "&6&lEsquinas del cuadrado (deben ser Warzone):");
    for (cx, cz) in [(-r, -r), (-r, r), (r, -r), (r, r)] {
        let t = core::territory_at(&world, cx, cz);
        let ok = matches!(t, core::Territory::Warzone);
        let viewer = core::faction_id_of(uuid);
        let d = core::territory_display(&t, viewer.as_deref());
        reply(sender, format!(" &e({cx}, {cz}) -> {d}  {}", if ok { "&aOK" } else { "&cFALLO" }));
    }

    reply(sender, "&6&l1 bloque fuera (deben ser Wilderness):");
    for (cx, cz) in [(r + 1, 0), (-(r + 1), 0), (0, r + 1), (0, -(r + 1))] {
        let t = core::territory_at(&world, cx, cz);
        let ok = matches!(t, core::Territory::Wilderness);
        let viewer = core::faction_id_of(uuid);
        let d = core::territory_display(&t, viewer.as_deref());
        reply(sender, format!(" &e({cx}, {cz}) -> {d}  {}", if ok { "&aOK" } else { "&cFALLO" }));
    }
    reply(sender, &sep);
}

fn set_home(sender: &CommandSender, player: &Player, uuid: &str) {
    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();
        if faction.member(uuid).map(|m| m.role).is_none_or(|role| !role.is_at_least(Role::CoLeader)) {
            reply(sender, &lang.faction_leader_only);
            return;
        }
        let home = core::location(player);
        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            faction.home = Some(home);
        }
        messages = lang.faction_home_set.clone();
    }
    reply(sender, messages);
}

fn home(sender: &CommandSender, player: &Player, uuid: &str) {
    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let Some(home) = guard.factions.get(&faction_id).and_then(|f| f.home.clone()) else {
            reply(sender, &lang.faction_home_not_set);
            return;
        };

        let warmup = match core::dimension_of(&home.world) {
            core::Dimension::Overworld => guard.config.home_warmup.overworld,
            core::Dimension::Nether => guard.config.home_warmup.nether,
            core::Dimension::End => guard.config.home_warmup.end,
        };

        let origin = core::location(player);
        let expiry = crate::util::now_millis() + (warmup as i64 * 1000);
        guard.timers.set_pending(
            uuid,
            PendingAction {
                timer_type: TimerType::Home,
                expiry,
                origin_world: origin.world,
                origin_x: origin.x,
                origin_y: origin.y,
                origin_z: origin.z,
                target: Some(home),
            },
        );

        messages = util::replace(&lang.faction_home_teleport, "<seconds>", &warmup.to_string());
    }
    reply(sender, messages);
}

fn stuck(sender: &CommandSender, player: &Player, uuid: &str) {
    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();
        let warmup = guard.config.timers.stuck_warmup_seconds;
        let origin = core::location(player);

        guard.timers.set_pending(
            uuid,
            PendingAction {
                timer_type: TimerType::Stuck,
                expiry: crate::util::now_millis() + (warmup as i64 * 1000),
                origin_world: origin.world.clone(),
                origin_x: origin.x,
                origin_y: origin.y,
                origin_z: origin.z,
                target: None,
            },
        );

        messages = util::replace(
            &lang.stuck_warmup,
            "<time>",
            &util::format_duration(warmup as i64 * 1000),
        );
    }
    reply(sender, messages);
}

fn toggle_open(sender: &CommandSender, server: &Server, uuid: &str) {
    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();

        let Some(faction_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = guard.factions.get(&faction_id).cloned().unwrap();
        if !is_leader(&faction, uuid) {
            reply(sender, &lang.faction_leader_only);
            return;
        }

        let new_value = !faction.open;
        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            faction.open = new_value;
        }
        messages = if new_value {
            util::replace(&lang.faction_opened, "<name>", &faction.name)
        } else {
            util::replace(&lang.faction_closed, "<name>", &faction.name)
        };
        let _ = server;
    }
    reply(sender, messages);
}

fn show(sender: &CommandSender, server: &Server, uuid: &str, name: Option<String>) {
    let guard = state().read().unwrap();
    let lang = guard.lang.clone();

    let faction = match &name {
        Some(name) => guard.factions.by_name(name).cloned(),
        None => guard.factions.by_member(uuid).cloned(),
    };

    let Some(faction) = faction else {
        reply(sender, &lang.faction_not_in);
        return;
    };

    // Relation colour for the faction name (as seen by the sender).
    let display = {
        let viewer = guard.factions.by_member(uuid);
        let color = match viewer {
            Some(v) if v.id == faction.id => &guard.config.relation_colors.teammate,
            Some(v) if v.is_ally(&faction.id) => &guard.config.relation_colors.ally,
            _ => &guard.config.relation_colors.enemy,
        };
        format!("{color}{}", faction.name)
    };

    let online = faction
        .members
        .values()
        .filter(|m| {
            pumpkin_plugin_api::uuid::parse(&m.uuid)
                .and_then(|id| server.get_player_by_uuid(id))
                .is_some()
        })
        .count();

    let role_names = |role: Role| -> String {
        let names: Vec<String> = faction
            .members
            .values()
            .filter(|m| m.role == role)
            .map(|m| format!("{}{}", guard.config.relation_colors.teammate, m.name))
            .collect();
        if names.is_empty() { lang.none.clone() } else { names.join("&7, ") }
    };

    let allies: Vec<String> = faction
        .allies
        .iter()
        .filter_map(|id| guard.factions.get(id).map(|f| f.name.clone()))
        .collect();
    let allies = if allies.is_empty() { lang.none.clone() } else { allies.join("&7, &f") };

    let max_dtr = faction.max_dtr(&guard.config);
    let dtr_color = if faction.dtr <= 0.0 { "&c" } else { "&a" };
    let raidable = if faction.is_raidable() { lang.yes.clone() } else { lang.no.clone() };

    let sep = format!("&7&m{}", " ".repeat(80));

    reply(sender, &sep);
    reply(sender, format!(" {display} &7[{online}/{}] &e- &eHome: &7{}", faction.members.len(), faction.home_string()));
    reply(sender, format!(" &eAnnouncement: &7{}", faction.announcement.clone().unwrap_or_else(|| lang.none.clone())));
    reply(sender, format!(" &eAuto revive: &7{}", lang.no));
    reply(sender, format!(" &eFaction lives: &7{}", faction.lives));
    reply(sender, format!(" &eTotal Kills: &7{}", faction.kills));
    reply(sender, format!(" &ePoints: &7{}", faction.points));
    reply(sender, format!(" &eKoTH captures: &7{}", faction.koths_capped));
    reply(sender, format!(" &eBalance: &7${}", faction.balance));
    reply(sender, format!(" &eLeader: {}", role_names(Role::Leader)));
    reply(sender, format!(" &eCo-leaders: {}", role_names(Role::CoLeader)));
    reply(sender, format!(" &eCaptains: {}", role_names(Role::Captain)));
    reply(sender, format!(" &eMembers: {}", role_names(Role::Member)));
    reply(sender, format!(" &eAllies: {allies}"));
    reply(sender, format!(" &eRegenerating: &7{}", lang.none));
    reply(sender, format!(" &eDTR: {dtr_color}{:.2}&7/&a{max_dtr:.2} &eRaidable: {raidable}", faction.dtr));
    reply(sender, &sep);
}

fn list(sender: &CommandSender) {
    let guard = state().read().unwrap();
    reply(sender, "&3=== &bFactions&3 ===");
    for faction in guard.factions.player_factions() {
        reply(
            sender,
            format!("&f{} &7- &e{} members &7- &c{:.2} DTR", faction.name, faction.members.len(), faction.dtr),
        );
    }
}

fn top(sender: &CommandSender, metric: &str) {
    let guard = state().read().unwrap();
    let mut factions: Vec<&Faction> = guard.factions.player_factions().collect();
    factions.sort_by(|a, b| match metric.to_lowercase().as_str() {
        "kills" => b.kills.cmp(&a.kills),
        "balance" => b.balance.cmp(&a.balance),
        "koths" => b.koths_capped.cmp(&a.koths_capped),
        _ => b.points.cmp(&a.points),
    });

    reply(sender, format!("&3=== &bTop factions ({metric})&3 ==="));
    for (index, faction) in factions.into_iter().take(10).enumerate() {
        let value = match metric.to_lowercase().as_str() {
            "kills" => faction.kills as i64,
            "balance" => faction.balance,
            "koths" => faction.koths_capped as i64,
            _ => faction.points as i64,
        };
        reply(sender, format!("&f{}. &e{} &7- &c{value}", index + 1, faction.name));
    }
}

fn transfer_balance(sender: &CommandSender, uuid: &str, args: &ConsumedArgs, deposit: bool) {
    let raw = string_arg(args, "amount").unwrap_or_default();

    let messages;
    {
        let mut guard = state().write().unwrap();

        // Destructure so the borrow checker sees disjoint fields (DerefMut would
        // otherwise borrow the whole guard through `guard.userdata`).
        let crate::state::AppState { userdata, factions, config, lang, .. } = &mut *guard;

        let max_balance = config.economy.max_balance;

        let Some(faction_id) = factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let faction = factions.get(&faction_id).cloned().unwrap();

        if !deposit
            && faction.member(uuid).map(|m| m.role).is_none_or(|role| !role.is_at_least(Role::Captain))
        {
            reply(sender, &lang.faction_leader_only);
            return;
        }

        let Some(user) = userdata.get_mut(uuid) else {
            return;
        };

        // Support `/f deposit all` / `/f withdraw all` like the original.
        let amount = if raw.eq_ignore_ascii_case("all") {
            if deposit { user.balance } else { faction.balance }
        } else {
            raw.parse::<i64>().unwrap_or(0)
        };

        if amount <= 0 {
            let usage = if deposit { lang.usage_deposit.clone() } else { lang.usage_withdraw.clone() };
            reply(sender, usage);
            return;
        }

        if deposit {
            if user.balance < amount {
                reply(sender, &lang.faction_not_enough_money);
                return;
            }
            user.balance -= amount;
            if let Some(faction) = factions.get_mut(&faction_id) {
                faction.balance = (faction.balance + amount).min(max_balance);
            }
            messages = util::replace(
                &util::replace(&lang.faction_deposit_deposited, "<player>", &sender.get_name()),
                "<amount>",
                &amount.to_string(),
            );
        } else {
            if faction.balance < amount {
                reply(
                    sender,
                    util::replace(&lang.faction_withdraw_not_enough, "<amount>", &faction.balance.to_string()),
                );
                return;
            }
            if let Some(faction) = factions.get_mut(&faction_id) {
                faction.balance -= amount;
            }
            user.balance = (user.balance + amount).min(max_balance);
            messages = util::replace(
                &util::replace(&lang.faction_withdraw_withdrawn, "<player>", &sender.get_name()),
                "<amount>",
                &amount.to_string(),
            );
        }
    }
    reply(sender, messages);
}

fn ally(sender: &CommandSender, server: &Server, uuid: &str, args: &ConsumedArgs, add: bool) {
    let Some(name) = string_arg(args, "faction") else {
        let usage = if add {
            state().read().unwrap().lang.usage_ally.clone()
        } else {
            state().read().unwrap().lang.usage_unally.clone()
        };
        reply(sender, usage);
        return;
    };

    let messages;
    {
        let mut guard = state().write().unwrap();
        let lang = guard.lang.clone();
        let max_allies = guard.config.factions.max_allies;

        let Some(my_id) = guard.factions.by_member(uuid).map(|f| f.id.clone()) else {
            reply(sender, &lang.faction_not_in);
            return;
        };
        let Some(other_id) = guard.factions.by_name(&name).filter(|f| f.is_player()).map(|f| f.id.clone()) else {
            reply(sender, util::replace(&lang.faction_doesnt_exist, "<argument>", &name));
            return;
        };
        if my_id == other_id {
            return;
        }

        if add {
            let ally_count = guard.factions.get(&my_id).map(|f| f.allies.len()).unwrap_or(0);
            if max_allies > 0 && ally_count >= max_allies {
                reply(sender, &lang.faction_ally_limit);
                return;
            }
            if let Some(faction) = guard.factions.get_mut(&my_id)
                && !faction.allies.contains(&other_id)
            {
                faction.allies.push(other_id.clone());
            }
            if let Some(faction) = guard.factions.get_mut(&other_id)
                && !faction.allies.contains(&my_id)
            {
                faction.allies.push(my_id.clone());
            }
            messages = util::replace(&lang.faction_ally_added, "<faction>", &name);
        } else {
            if let Some(faction) = guard.factions.get_mut(&my_id) {
                faction.allies.retain(|id| id != &other_id);
            }
            if let Some(faction) = guard.factions.get_mut(&other_id) {
                faction.allies.retain(|id| id != &my_id);
            }
            messages = util::replace(&lang.faction_ally_removed, "<faction>", &name);
        }
        let _ = server;
    }
    reply(sender, messages);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_leader(faction: &Faction, uuid: &str) -> bool {
    faction.member(uuid).map(|m| m.role) == Some(Role::Leader)
}

fn is_connected(claims: &crate::factions::ClaimManager, faction_id: &str, world: &str, cx: i32, cz: i32) -> bool {
    let neighbors = [
        (cx + 1, cz),
        (cx - 1, cz),
        (cx, cz + 1),
        (cx, cz - 1),
        (cx + 1, cz + 1),
        (cx + 1, cz - 1),
        (cx - 1, cz + 1),
        (cx - 1, cz - 1),
    ];
    neighbors.iter().any(|(nx, nz)| {
        claims
            .faction_at(world, nx * 16, nz * 16)
            .map(|owner| owner == faction_id)
            .unwrap_or(false)
    })
}

// ---------------------------------------------------------------------------
// Admin subcommands
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum AdminField {
    Points,
    Balance,
    Lives,
}

#[derive(Clone, Copy)]
enum SystemFlag {
    Safezone,
    Enderpearls,
    Abilities,
}

/// Per-subcommand permission check (the original registers one
/// `lazarus.factions.*` node per admin subcommand).
fn require_permission(sender: &CommandSender, server: &Server, node: &str) -> bool {
    if sender.is_console() || sender.has_permission(server, node) {
        return true;
    }
    reply(sender, "&cYou do not have permission.");
    false
}

fn admin_change_dtr(sender: &CommandSender, server: &Server, args: &ConsumedArgs, add: bool) {
    if !require_permission(sender, server, "lazarus:factions.dtr") {
        return;
    }
    let Some(name) = string_arg(args, "faction") else {
        return;
    };
    let Some(value) = double_arg(args, "value") else {
        let message = state().read().unwrap().raw.language.string("COMMANDS.INVALID_NUMBER");
        reply(sender, message);
        return;
    };

    let mut guard = state().write().unwrap();
    let config = guard.config.clone();
    let prefix = guard.raw.language.string("FACTION_PREFIX");
    let sender_tpl = guard.raw.language.string(if add {
        "FACTIONS.ADD_DTR_COMMAND.CHANGED_SENDER"
    } else {
        "FACTIONS.REMOVE_DTR_COMMAND.CHANGED_SENDER"
    });
    let faction_tpl = guard.raw.language.string(if add {
        "FACTIONS.ADD_DTR_COMMAND.CHANGED_FACTION"
    } else {
        "FACTIONS.REMOVE_DTR_COMMAND.CHANGED_FACTION"
    });
    let missing_tpl = guard.raw.language.string("FACTIONS.FACTION_DOESNT_EXIST");

    let Some(id) = guard
        .factions
        .by_name(&name)
        .filter(|f| f.is_player())
        .map(|f| f.id.clone())
    else {
        let message = util::replace(&missing_tpl, "<argument>", &name);
        reply(sender, format!("{prefix}{message}"));
        return;
    };

    let (faction_name, members, max, current) = {
        let faction = guard.factions.get(&id).unwrap();
        (
            faction.name.clone(),
            faction.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>(),
            faction.max_dtr(&config),
            faction.dtr,
        )
    };

    let amount = if add { current + value } else { current - value };
    let clamped = if amount < 0.0 {
        amount.max(config.factions.min_dtr)
    } else {
        amount.min(max)
    };
    let new_dtr = (clamped * 100.0).round() / 100.0;
    if let Some(faction) = guard.factions.get_mut(&id) {
        faction.dtr = new_dtr;
    }

    let color = if new_dtr <= 0.0 {
        "&c"
    } else if new_dtr <= 1.0 {
        "&e"
    } else {
        "&a"
    };
    let character = if new_dtr >= max {
        guard.raw.config.string("FACTION_DTR.DTR_CHARACTERS.FULL_DTR")
    } else {
        guard.raw.config.string("FACTION_DTR.DTR_CHARACTERS.REGENERATING")
    };
    let number = if new_dtr == new_dtr.trunc() {
        format!("{new_dtr:.1}")
    } else {
        format!("{new_dtr}")
    };
    let dtr_string = format!("{color}{number}{character}");

    let sender_message = format!(
        "{prefix}{}",
        util::replace_all(&sender_tpl, &[("<faction>", &faction_name), ("<value>", &dtr_string)])
    );
    let faction_message = format!(
        "{prefix}{}",
        util::replace_all(
            &faction_tpl,
            &[("<player>", &sender.get_name()), ("<dtr>", &dtr_string)]
        )
    );

    for uuid in members {
        if let Some(id) = pumpkin_plugin_api::uuid::parse(&uuid)
            && let Some(player) = server.get_player_by_uuid(id)
        {
            core::send(&player, &faction_message);
        }
    }

    reply(sender, sender_message);
}

fn admin_force_role(sender: &CommandSender, server: &Server, args: &ConsumedArgs, promote: bool) {
    if !require_permission(sender, server, if promote { "lazarus:factions.forcepromote" } else { "lazarus:factions.forcedemote" }) {
        return;
    }
    let Some(target_name) = string_arg(args, "player") else {
        return;
    };

    let (prefix, usage, not_in_tpl, max_tpl, min_tpl, leader_tpl, sender_tpl, faction_tpl) = {
        let guard = state().read().unwrap();
        let lang = &guard.raw.language;
        let (usage, max_tpl, min_tpl, leader_tpl, sender_tpl, faction_tpl) = if promote {
            (
                lang.string("FACTIONS.FORCE_PROMOTE_COMMAND.USAGE"),
                lang.string("FACTIONS.FORCE_PROMOTE_COMMAND.MAX_PROMOTE"),
                String::new(),
                String::new(),
                lang.string("FACTIONS.FORCE_PROMOTE_COMMAND.PROMOTED_SENDER"),
                lang.string("FACTIONS.FORCE_PROMOTE_COMMAND.PROMOTED_FACTION"),
            )
        } else {
            (
                lang.string("FACTIONS.FORCE_DEMOTE_COMMAND.USAGE"),
                String::new(),
                lang.string("FACTIONS.FORCE_DEMOTE_COMMAND.MIN_DEMOTE"),
                lang.string("FACTIONS.FORCE_DEMOTE_COMMAND.CANNOT_DEMOTE_LEADER"),
                lang.string("FACTIONS.FORCE_DEMOTE_COMMAND.DEMOTED_SENDER"),
                lang.string("FACTIONS.FORCE_DEMOTE_COMMAND.DEMOTED_FACTION"),
            )
        };
        (
            lang.string("FACTION_PREFIX"),
            usage,
            lang.string("FACTIONS.NOT_IN_FACTION"),
            max_tpl,
            min_tpl,
            leader_tpl,
            sender_tpl,
            faction_tpl,
        )
    };
    let _ = usage;

    // Locate the member by name across all player factions.
    let found = {
        let guard = state().read().unwrap();
        guard.factions.player_factions().find_map(|faction| {
            faction
                .members
                .values()
                .find(|m| m.name.eq_ignore_ascii_case(&target_name))
                .map(|m| (faction.id.clone(), m.uuid.clone(), m.role, faction.name.clone()))
        })
    };

    let Some((faction_id, member_uuid, role, faction_name)) = found else {
        let message = util::replace(&not_in_tpl, "<player>", &target_name);
        reply(sender, format!("{prefix}{message}"));
        return;
    };

    let new_role = if promote {
        match role.promote() {
            Some(role) => role,
            None => {
                reply(sender, format!("{prefix}{max_tpl}"));
                return;
            }
        }
    } else {
        if role == Role::Leader {
            reply(sender, format!("{prefix}{leader_tpl}"));
            return;
        }
        match role.demote() {
            Some(role) => role,
            None => {
                reply(sender, format!("{prefix}{min_tpl}"));
                return;
            }
        }
    };

    let members = {
        let mut guard = state().write().unwrap();
        if let Some(faction) = guard.factions.get_mut(&faction_id) {
            if let Some(member) = faction.member_mut(&member_uuid) {
                member.role = new_role;
            }
            faction.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>()
        } else {
            Vec::new()
        }
    };

    let role_name = new_role.name();
    reply(
        sender,
        format!(
            "{prefix}{}",
            util::replace_all(
                &sender_tpl,
                &[("<player>", &target_name), ("<role>", role_name)]
            )
        ),
    );

    let message = format!(
        "{prefix}{}",
        util::replace_all(
            &faction_tpl,
            &[("<player>", &target_name), ("<role>", role_name)]
        )
    );
    let _ = faction_name;
    for uuid in members {
        if let Some(id) = pumpkin_plugin_api::uuid::parse(&uuid)
            && let Some(player) = server.get_player_by_uuid(id)
        {
            core::send(&player, &message);
        }
    }
}

fn admin_force_join(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_permission(sender, server, "lazarus:factions.forcejoin") {
        return;
    }
    let Some(faction_name) = string_arg(args, "faction") else {
        return;
    };
    let Some(player) = sender.as_player() else {
        return;
    };
    let uuid = core::uuid_of(&player);
    let name = core::name_of(&player);

    let (prefix, missing_tpl, already_tpl, joined_tpl, already, found) = {
        let guard = state().read().unwrap();
        let lang = &guard.raw.language;
        let found = guard
            .factions
            .by_name(&faction_name)
            .filter(|f| f.is_player())
            .map(|f| {
                (
                    f.id.clone(),
                    f.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>(),
                )
            });
        (
            lang.string("FACTION_PREFIX"),
            lang.string("FACTIONS.FACTION_DOESNT_EXIST"),
            lang.string("FACTIONS.ALREADY_IN_FACTION_SELF"),
            lang.string("FACTIONS.FORCE_JOIN_COMMAND.JOINED"),
            guard.factions.by_member(&uuid).is_some(),
            found,
        )
    };

    if already {
        reply(sender, format!("{prefix}{already_tpl}"));
        return;
    }
    let Some((id, members)) = found else {
        let message = util::replace(&missing_tpl, "<argument>", &faction_name);
        reply(sender, format!("{prefix}{message}"));
        return;
    };

    {
        let mut guard = state().write().unwrap();
        guard
            .factions
            .add_member(&id, member(uuid.clone(), name.clone(), Role::Member));
    }

    let message = format!(
        "{prefix}{}",
        util::replace(&joined_tpl, "<player>", &name)
    );
    for member_uuid in members {
        if let Some(id) = pumpkin_plugin_api::uuid::parse(&member_uuid)
            && let Some(online) = server.get_player_by_uuid(id)
        {
            core::send(&online, &message);
        }
    }
}

fn admin_force_rename(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let _ = server;
    if !require_permission(sender, server, "lazarus:factions.forcerename") {
        return;
    }
    let Some(old_arg) = string_arg(args, "faction") else {
        return;
    };
    let Some(new_name) = string_arg(args, "name") else {
        return;
    };

    let mut guard = state().write().unwrap();
    let config = guard.config.clone();

    let (prefix, usage, missing_tpl, same_tpl, blocked, too_short, too_big, not_alnum, exists_tpl, renamed_tpl) = {
        let lang = &guard.raw.language;
        (
            lang.string("FACTION_PREFIX"),
            lang.string("FACTIONS.FORCE_RENAME_COMMAND.USAGE"),
            lang.string("FACTIONS.FACTION_DOESNT_EXIST"),
            lang.string("FACTIONS.FORCE_RENAME_COMMAND.SAME_NAME"),
            lang.string("FACTIONS.BLOCKED_FACTION_NAME"),
            lang.string("FACTIONS.NAME_TOO_SHORT"),
            lang.string("FACTIONS.NAME_TOO_BIG"),
            lang.string("FACTIONS.NAME_NOT_ALPHANUMERIC"),
            lang.string("FACTIONS.FACTION_ALREADY_EXISTS"),
            lang.string("FACTIONS.FORCE_RENAME_COMMAND.RENAMED"),
        )
    };
    let _ = usage;

    let Some(id) = guard
        .factions
        .by_name(&old_arg)
        .filter(|f| f.is_player())
        .map(|f| f.id.clone())
    else {
        let message = util::replace(&missing_tpl, "<argument>", &old_arg);
        reply(sender, format!("{prefix}{message}"));
        return;
    };

    let old_name = guard.factions.get(&id).map(|f| f.name.clone()).unwrap_or_default();

    if old_name.eq_ignore_ascii_case(&new_name) {
        reply(sender, format!("{prefix}{}", util::replace(&same_tpl, "<name>", &new_name)));
        return;
    }
    if config
        .factions
        .disallowed_names
        .iter()
        .any(|n| n.eq_ignore_ascii_case(&new_name))
    {
        reply(sender, format!("{prefix}{blocked}"));
        return;
    }
    if new_name.chars().count() < config.factions.name_min_length {
        reply(sender, format!("{prefix}{too_short}"));
        return;
    }
    if new_name.chars().count() > config.factions.name_max_length {
        reply(sender, format!("{prefix}{too_big}"));
        return;
    }
    if !new_name.chars().all(|c| c.is_ascii_alphanumeric()) {
        reply(sender, format!("{prefix}{not_alnum}"));
        return;
    }
    if guard.factions.name_taken(&new_name) {
        reply(sender, format!("{prefix}{}", util::replace(&exists_tpl, "<name>", &new_name)));
        return;
    }

    if let Some(faction) = guard.factions.get_mut(&id) {
        faction.name = new_name.clone();
    }
    drop(guard);

    let message = util::replace_all(
        &renamed_tpl,
        &[("<name>", &old_name), ("<newName>", &new_name)],
    );
    for player in server.get_all_players() {
        core::send(&player, &message);
    }
}

fn admin_system_create(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let _ = server;
    if !require_permission(sender, server, "lazarus:factions.systemcreate") {
        return;
    }
    let Some(name) = string_arg(args, "name") else {
        return;
    };

    let mut guard = state().write().unwrap();
    let prefix = guard.raw.language.string("FACTION_PREFIX");
    if guard.factions.name_taken(&name) {
        let tpl = guard.raw.language.string("FACTIONS.FACTION_ALREADY_EXISTS");
        drop(guard);
        reply(sender, format!("{prefix}{}", util::replace(&tpl, "<name>", &name)));
        return;
    }
    let created = guard.raw.language.string("FACTIONS.SYSTEM_CREATE_COMMAND.CREATED");

    let id = crate::factions::new_id();
    let faction = Faction::new_system(
        id.clone(),
        name.clone(),
        crate::factions::FactionKind::System,
        "&7".to_string(),
    );
    guard.factions.factions.insert(id, faction);
    drop(guard);

    reply(
        sender,
        format!("{prefix}{}", util::replace(&created, "<name>", &name)),
    );
}

fn admin_system_list(sender: &CommandSender) {
    fn kind_order(kind: crate::factions::FactionKind) -> u8 {
        match kind {
            crate::factions::FactionKind::System => 0,
            crate::factions::FactionKind::Safezone => 1,
            crate::factions::FactionKind::Warzone => 2,
            crate::factions::FactionKind::Wilderness => 3,
            crate::factions::FactionKind::Road => 4,
            crate::factions::FactionKind::Player => 5,
        }
    }

    let (header, format_tpl, footer, factions) = {
        let guard = state().read().unwrap();
        let lang = &guard.raw.language;
        let mut list: Vec<(u8, String, String)> = guard
            .factions
            .factions
            .values()
            .filter(|f| f.is_system())
            .map(|f| (kind_order(f.kind), f.color.clone(), f.name.clone()))
            .collect();
        list.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.2.cmp(&b.2)));
        (
            lang.string_list("FACTIONS.SYSTEM_LIST_COMMAND.HEADER"),
            lang.string("FACTIONS.SYSTEM_LIST_COMMAND.FACTION_FORMAT"),
            lang.string("FACTIONS.SYSTEM_LIST_COMMAND.FOOTER"),
            list,
        )
    };

    let count = factions.len().to_string();
    for line in &header {
        reply(sender, util::replace(line, "<count>", &count));
    }
    for (index, (_, color, name)) in factions.iter().enumerate() {
        let number = (index + 1).to_string();
        let display = format!("{color}{name}");
        reply(
            sender,
            util::replace_all(
                &format_tpl,
                &[("<number>", &number), ("<name>", &display)],
            ),
        );
    }
    reply(sender, footer);
}

fn admin_system_rename(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let _ = server;
    if !require_permission(sender, server, "lazarus:factions.systemrename") {
        return;
    }
    let Some(old_arg) = string_arg(args, "faction") else {
        return;
    };
    let Some(new_name) = string_arg(args, "name") else {
        return;
    };

    let mut guard = state().write().unwrap();
    let prefix = guard.raw.language.string("FACTION_PREFIX");
    let missing_tpl = guard.raw.language.string("FACTIONS.FACTION_DOESNT_EXIST");
    let not_system = guard.raw.language.string("FACTIONS.SYSTEM_RENAME_COMMAND.NOT_SYSTEM_FACTION");
    let cannot = guard
        .raw
        .language
        .string("FACTIONS.SYSTEM_RENAME_COMMAND.CANNOT_RENAME_THIS_TYPE");
    let same_tpl = guard.raw.language.string("FACTIONS.SYSTEM_RENAME_COMMAND.SAME_NAME");
    let exists_tpl = guard.raw.language.string("FACTIONS.FACTION_ALREADY_EXISTS");
    let renamed_tpl = guard.raw.language.string("FACTIONS.SYSTEM_RENAME_COMMAND.RENAMED");

    let Some(id) = guard.factions.by_name(&old_arg).map(|f| f.id.clone()) else {
        reply(sender, format!("{prefix}{}", util::replace(&missing_tpl, "<argument>", &old_arg)));
        return;
    };

    let Some(faction) = guard.factions.get(&id) else {
        return;
    };
    let kind = faction.kind;
    let old_name = faction.name.clone();
    let color = faction.color.clone();

    if kind == crate::factions::FactionKind::Player {
        reply(sender, format!("{prefix}{not_system}"));
        return;
    }
    if kind != crate::factions::FactionKind::System {
        reply(sender, format!("{prefix}{cannot}"));
        return;
    }
    if old_name.eq_ignore_ascii_case(&new_name) {
        reply(sender, format!("{prefix}{}", util::replace(&same_tpl, "<name>", &new_name)));
        return;
    }
    if guard.factions.name_taken(&new_name) {
        reply(sender, format!("{prefix}{}", util::replace(&exists_tpl, "<name>", &new_name)));
        return;
    }

    if let Some(faction) = guard.factions.get_mut(&id) {
        faction.name = new_name.clone();
    }
    drop(guard);

    reply(
        sender,
        format!(
            "{prefix}{}",
            util::replace_all(
                &renamed_tpl,
                &[("<faction>", &format!("{color}{old_name}")), ("<name>", &format!("{color}{new_name}"))],
            )
        ),
    );
}

fn admin_toggle_system_flag(
    sender: &CommandSender,
    server: &Server,
    args: &ConsumedArgs,
    flag: SystemFlag,
) {
    let _ = server;
    let node = match flag {
        SystemFlag::Safezone => "lazarus:factions.safezone",
        SystemFlag::Enderpearls => "lazarus:factions.enderpearls",
        SystemFlag::Abilities => "lazarus:factions.abilities",
    };
    if !require_permission(sender, server, node) {
        return;
    }
    let Some(name) = string_arg(args, "faction") else {
        return;
    };

    let mut guard = state().write().unwrap();
    let lang = &guard.raw.language;
    let prefix = lang.string("FACTION_PREFIX");
    let usage = lang.string(match flag {
        SystemFlag::Safezone => "FACTIONS.SAFEZONE_COMMAND.USAGE",
        SystemFlag::Enderpearls => "FACTIONS.ENDERPEARLS_COMMAND.USAGE",
        SystemFlag::Abilities => "FACTIONS.ABILITIES_COMMAND.USAGE",
    });
    let missing_tpl = lang.string("FACTIONS.FACTION_DOESNT_EXIST");
    let not_system = lang.string(match flag {
        SystemFlag::Safezone => "FACTIONS.SAFEZONE_COMMAND.NOT_SYSTEM_FACTION",
        SystemFlag::Enderpearls => "FACTIONS.ENDERPEARLS_COMMAND.NOT_SYSTEM_FACTION",
        SystemFlag::Abilities => "FACTIONS.ABILITIES_COMMAND.NOT_SYSTEM_FACTION",
    });

    let Some(id) = guard.factions.by_name(&name).map(|f| f.id.clone()) else {
        let message = util::replace(&missing_tpl, "<argument>", &name);
        reply(sender, format!("{prefix}{message}"));
        return;
    };

    let Some(faction) = guard.factions.get(&id) else {
        return;
    };
    if !faction.is_system() {
        reply(sender, format!("{prefix}{not_system}"));
        return;
    }
    let display = format!("{}{}", faction.color, faction.name);

    // Toggle and render the option-specific message.
    let message = match flag {
        SystemFlag::Safezone => {
            let faction = guard.factions.get_mut(&id).unwrap();
            faction.safezone = !faction.safezone;
            let now = faction.safezone;
            let tpl = guard.raw.language.string("FACTIONS.SAFEZONE_COMMAND.CHANGED");
            util::replace_all(
                &tpl,
                &[
                    ("<faction>", &display),
                    ("<safezone>", if now { "&atrue" } else { "&cfalse" }),
                ],
            )
        }
        SystemFlag::Enderpearls => {
            let faction = guard.factions.get_mut(&id).unwrap();
            faction.enderpearls = !faction.enderpearls;
            let now = faction.enderpearls;
            let tpl = guard.raw.language.string(if now {
                "FACTIONS.ENDERPEARLS_COMMAND.ENABLED"
            } else {
                "FACTIONS.ENDERPEARLS_COMMAND.DISABLED"
            });
            util::replace(&tpl, "<faction>", &display)
        }
        SystemFlag::Abilities => {
            let faction = guard.factions.get_mut(&id).unwrap();
            faction.abilities = !faction.abilities;
            let now = faction.abilities;
            let tpl = guard.raw.language.string(if now {
                "FACTIONS.ABILITIES_COMMAND.ENABLED"
            } else {
                "FACTIONS.ABILITIES_COMMAND.DISABLED"
            });
            util::replace(&tpl, "<faction>", &display)
        }
    };
    let _ = usage;
    drop(guard);

    reply(sender, format!("{prefix}{message}"));
}

fn admin_color(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_permission(sender, server, "lazarus:factions.color") {
        return;
    }
    let Some(name) = string_arg(args, "faction") else {
        return;
    };
    let Some(color) = string_arg(args, "color") else {
        return;
    };

    let mut guard = state().write().unwrap();
    let lang = &guard.raw.language;
    let prefix = lang.string("FACTION_PREFIX");
    let missing_tpl = lang.string("FACTIONS.FACTION_DOESNT_EXIST");
    let not_system = lang.string("FACTIONS.COLOR_COMMAND.NOT_SYSTEM_FACTION");
    let invalid_tpl = lang.string("FACTIONS.COLOR_COMMAND.COLOR_DOESNT_EXIST");
    let changed_tpl = lang.string("FACTIONS.COLOR_COMMAND.CHANGED");

    let Some(id) = guard.factions.by_name(&name).map(|f| f.id.clone()) else {
        let message = util::replace(&missing_tpl, "<argument>", &name);
        reply(sender, format!("{prefix}{message}"));
        return;
    };

    let Some(faction) = guard.factions.get(&id) else {
        return;
    };
    if !faction.is_system() {
        reply(sender, format!("{prefix}{not_system}"));
        return;
    }

    let lowered = color.to_lowercase();
    if !is_valid_color(&lowered) {
        reply(sender, format!("{prefix}{}", util::replace(&invalid_tpl, "<color>", &color)));
        return;
    }

    let old_display = format!("{}{}", faction.color, faction.name);
    if let Some(faction) = guard.factions.get_mut(&id) {
        faction.color = lowered.clone();
    }
    let new_display = format!("{}{}", lowered, guard.factions.get(&id).map(|f| f.name.clone()).unwrap_or_default());

    reply(
        sender,
        format!(
            "{prefix}{}",
            util::replace_all(&changed_tpl, &[("<faction>", &old_display), ("<color>", &new_display)])
        ),
    );
}

/// Port of `ColorCommand#isColorStringValid`.
fn is_valid_color(value: &str) -> bool {
    if value.is_empty() || value.len() % 2 != 0 {
        return false;
    }
    let bytes = value.as_bytes();
    for pair in bytes.chunks(2) {
        if pair[0] != b'&' {
            return false;
        }
        let c = pair[1] as char;
        if !"0123456789abcdefklmnor".contains(c) {
            return false;
        }
    }
    true
}

fn admin_deathban(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let _ = server;
    if !require_permission(sender, server, "lazarus:factions.deathban") {
        return;
    }
    let Some(name) = string_arg(args, "faction") else {
        return;
    };

    let mut guard = state().write().unwrap();
    let lang = &guard.raw.language;
    let prefix = lang.string("FACTION_PREFIX");
    let missing_tpl = lang.string("FACTIONS.FACTION_DOESNT_EXIST");
    let not_system = lang.string("FACTIONS.DEATHBAN_COMMAND.NOT_SYSTEM_FACTION");
    let changed_tpl = lang.string("FACTIONS.DEATHBAN_COMMAND.CHANGED");

    let Some(id) = guard.factions.by_name(&name).map(|f| f.id.clone()) else {
        let message = util::replace(&missing_tpl, "<argument>", &name);
        reply(sender, format!("{prefix}{message}"));
        return;
    };

    let Some(faction) = guard.factions.get(&id) else {
        return;
    };
    if !faction.is_system() {
        reply(sender, format!("{prefix}{not_system}"));
        return;
    }
    let display = format!("{}{}", faction.color, faction.name);

    let Some(faction) = guard.factions.get_mut(&id) else {
        return;
    };
    faction.deathban = !faction.deathban;
    let deathban = if faction.deathban { "&cDeathban" } else { "&aNon-Deathban" };

    reply(
        sender,
        format!(
            "{prefix}{}",
            util::replace_all(&changed_tpl, &[("<faction>", &display), ("<deathban>", deathban)])
        ),
    );
}

fn admin_chat_spy(sender: &CommandSender, server: &Server) {
    let _ = server;
    if !require_permission(sender, server, "lazarus:factions.chatspy") {
        return;
    }
    let Some(player) = sender.as_player() else {
        return;
    };
    let uuid = core::uuid_of(&player);

    let (enabled, message) = {
        let mut guard = state().write().unwrap();
        let enabled = guard.factions.toggle_chat_spy(&uuid);
        let key = if enabled {
            "FACTIONS.CHATSPY_ENABLED"
        } else {
            "FACTIONS.CHATSPY_DISABLED"
        };
        let prefix = guard.raw.language.string("FACTION_PREFIX");
        let message = guard.raw.language.string(key);
        (enabled, format!("{prefix}{message}"))
    };
    let _ = enabled;
    reply(sender, message);
}

fn admin_tp_here(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_permission(sender, server, "lazarus:factions.tphere") {
        return;
    }
    let Some(name) = string_arg(args, "faction") else {
        return;
    };
    let Some(player) = sender.as_player() else {
        return;
    };

    let (prefix, missing_tpl, faction_tpl, sender_tpl, members, faction_name) = {
        let guard = state().read().unwrap();
        let lang = &guard.raw.language;
        let found = guard.factions.by_name(&name).filter(|f| f.is_player()).map(|f| {
            (
                f.name.clone(),
                f.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>(),
            )
        });
        let (faction_name, members) = found.unwrap_or_default();
        (
            lang.string("FACTION_PREFIX"),
            lang.string("FACTIONS.FACTION_DOESNT_EXIST"),
            lang.string("FACTIONS.TP_HERE_COMMAND.TELEPORTED_FACTION"),
            lang.string("FACTIONS.TP_HERE_COMMAND.TELEPORTED_SENDER"),
            members,
            faction_name,
        )
    };

    if faction_name.is_empty() {
        let message = util::replace(&missing_tpl, "<argument>", &name);
        reply(sender, format!("{prefix}{message}"));
        return;
    }

    let location = core::location(&player);
    let position = (location.x, location.y, location.z);

    for member_uuid in &members {
        if let Some(id) = pumpkin_plugin_api::uuid::parse(member_uuid)
            && let Some(online) = server.get_player_by_uuid(id)
        {
            if let Some(world) = server
                .get_all_worlds()
                .into_iter()
                .find(|w| w.get_name() == location.world)
            {
                online.teleport(position, None, None, world);
            }
            core::send(
                &online,
                format!("{prefix}{}", util::replace(&faction_tpl, "<player>", &core::name_of(&player))),
            );
        }
    }

    reply(
        sender,
        format!("{prefix}{}", util::replace(&sender_tpl, "<faction>", &faction_name)),
    );
}

fn admin_remove_claim(sender: &CommandSender, server: &Server) {
    let _ = server;
    if !require_permission(sender, server, "lazarus:factions.removeclaim") {
        return;
    }
    let Some(player) = sender.as_player() else {
        return;
    };
    let location = core::location(&player);

    let mut guard = state().write().unwrap();
    let lang_prefix = guard.raw.language.string("FACTION_PREFIX");

    let Some((owner_id, claim)) = guard
        .claims
        .claim_at(&location.world, location.block_x(), location.block_z())
        .map(|(owner, claim)| (owner.clone(), claim.clone()))
    else {
        let message = guard.raw.language.string("FACTIONS.REMOVE_CLAIM_COMMAND.NO_CLAIM");
        drop(guard);
        reply(sender, format!("{lang_prefix}{message}"));
        return;
    };

    let center = format!("{}, {}", (claim.min_x + claim.max_x) / 2, (claim.min_z + claim.max_z) / 2);
    let owner_display = guard
        .factions
        .get(&owner_id)
        .map(|f| format!("{}{}", f.color, f.name))
        .unwrap_or_default();
    let is_player = guard.factions.get(&owner_id).map(|f| f.is_player()).unwrap_or(false);

    guard
        .claims
        .remove_claim_at(&owner_id, &location.world, location.block_x(), location.block_z());

    // Clear the home if it was inside the removed claim.
    if is_player
        && claim.contains(&location.world, claim.min_x, claim.min_z)
        && let Some(faction) = guard.factions.get(&owner_id)
        && let Some(home) = &faction.home
        && claim.contains(&location.world, home.block_x(), home.block_z())
        && let Some(faction) = guard.factions.get_mut(&owner_id)
    {
        faction.home = None;
    }

    let sender_tpl = guard.raw.language.string("FACTIONS.REMOVE_CLAIM_COMMAND.REMOVED_SENDER");
    let faction_tpl = guard.raw.language.string("FACTIONS.REMOVE_CLAIM_COMMAND.REMOVED_FACTION");
    let members: Vec<String> = if is_player {
        guard
            .factions
            .get(&owner_id)
            .map(|f| f.members.values().map(|m| m.uuid.clone()).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    drop(guard);

    reply(
        sender,
        format!(
            "{lang_prefix}{}",
            util::replace_all(&sender_tpl, &[("<faction>", &owner_display), ("<location>", &center)])
        ),
    );

    if is_player {
        let player_name = core::name_of(&player);
        let message = format!(
            "{lang_prefix}{}",
            util::replace_all(&faction_tpl, &[("<player>", &player_name), ("<location>", &center)])
        );
        for member_uuid in members {
            if let Some(id) = pumpkin_plugin_api::uuid::parse(&member_uuid)
                && let Some(online) = server.get_player_by_uuid(id)
            {
                core::send(&online, &message);
            }
        }
    }
}

fn admin_claim_for(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let _ = server;
    if !require_permission(sender, server, "lazarus:factions.claimfor") {
        return;
    }
    let Some(name) = string_arg(args, "faction") else {
        return;
    };
    let Some(player) = sender.as_player() else {
        return;
    };
    let uuid = core::uuid_of(&player);

    let mut guard = state().write().unwrap();
    let prefix = guard.raw.language.string("FACTION_PREFIX");

    let Some(owner_id) = guard
        .factions
        .by_name(&name)
        .filter(|f| f.is_system())
        .map(|f| f.id.clone())
    else {
        let message = util::replace(
            &guard.raw.language.string("FACTIONS.FACTION_DOESNT_EXIST"),
            "<argument>",
            &name,
        );
        drop(guard);
        reply(sender, format!("{prefix}{message}"));
        return;
    };

    let Some(claim) = guard.selection.get(&uuid).and_then(|s| s.to_claim()) else {
        let message = guard.raw.language.string("FACTIONS.CLAIM_FOR_COMMAND.MAKE_A_SELECTION");
        drop(guard);
        reply(sender, format!("{prefix}{message}"));
        return;
    };

    if guard.claims.overlaps(&claim) {
        let message = guard.raw.language.string("FACTIONS.CLAIM_FOR_COMMAND.CLAIM_OVERLAPPING");
        drop(guard);
        reply(sender, format!("{prefix}{message}"));
        return;
    }

    guard.claims.add_claim(&owner_id, claim);
    let display = guard
        .factions
        .get(&owner_id)
        .map(|f| format!("{}{}", f.color, f.name))
        .unwrap_or_default();
    let message = util::replace_all(
        &guard.raw.language.string("FACTIONS.CLAIM_FOR_COMMAND.CLAIM_CLAIMED"),
        &[("<player>", &core::name_of(&player)), ("<faction>", &display)],
    );
    guard.selection.cancel_local(&player);
    drop(guard);

    reply(sender, format!("{prefix}{message}"));
}

fn admin_remove(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_permission(sender, server, "lazarus:factions.remove") {
        return;
    }

    let (prefix, usage, missing_tpl, invalid_type, sender_tpl, faction_tpl, found) = {
        let guard = state().read().unwrap();
        let lang = &guard.raw.language;
        let found = guard.factions.by_name(&string_arg(args, "faction").unwrap_or_default()).map(|f| {
            (
                f.id.clone(),
                f.name.clone(),
                f.is_player(),
                f.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>(),
            )
        });
        (
            lang.string("FACTION_PREFIX"),
            lang.string("FACTIONS.REMOVE_COMMAND.USAGE"),
            lang.string("FACTIONS.FACTION_DOESNT_EXIST"),
            lang.string("FACTIONS.REMOVE_COMMAND.CANNOT_REMOVE_THIS_TYPE"),
            lang.string("FACTIONS.REMOVE_COMMAND.REMOVED_SENDER"),
            lang.string("FACTIONS.REMOVE_COMMAND.REMOVED_FACTION"),
            found,
        )
    };

    let Some(name) = string_arg(args, "faction") else {
        reply(sender, format!("{prefix}{usage}"));
        return;
    };
    let Some((id, faction_name, is_player, members)) = found else {
        let message = util::replace(&missing_tpl, "<argument>", &name);
        reply(sender, format!("{prefix}{message}"));
        return;
    };
    if !is_player {
        reply(sender, format!("{prefix}{invalid_type}"));
        return;
    }

    {
        let mut guard = state().write().unwrap();
        guard.claims.remove_all(&id);
        guard.factions.disband(&id);
    }

    reply(
        sender,
        format!("{prefix}{}", util::replace(&sender_tpl, "<faction>", &faction_name)),
    );

    let message = format!(
        "{prefix}{}",
        util::replace(&faction_tpl, "<player>", &sender.get_name())
    );
    for uuid in members {
        if let Some(id) = pumpkin_plugin_api::uuid::parse(&uuid)
            && let Some(player) = server.get_player_by_uuid(id)
        {
            core::send(&player, &message);
        }
    }
}

fn admin_clear_claims(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_permission(sender, server, "lazarus:factions.clearclaims") {
        return;
    }

    let (prefix, usage, missing_tpl, sender_tpl, faction_tpl, home_tpl, found) = {
        let guard = state().read().unwrap();
        let lang = &guard.raw.language;
        let found = guard.factions.by_name(&string_arg(args, "faction").unwrap_or_default()).map(|f| {
            (
                f.id.clone(),
                f.name.clone(),
                f.is_player(),
                f.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>(),
            )
        });
        (
            lang.string("FACTION_PREFIX"),
            lang.string("FACTIONS.CLEAR_CLAIMS_COMMAND.USAGE"),
            lang.string("FACTIONS.FACTION_DOESNT_EXIST"),
            lang.string("FACTIONS.CLEAR_CLAIMS_COMMAND.CLEARED_SENDER"),
            lang.string("FACTIONS.CLEAR_CLAIMS_COMMAND.CLEARED_FACTION"),
            lang.string("FACTIONS.CLEAR_CLAIMS_COMMAND.HOME_TELEPORT_CANCELLED"),
            found,
        )
    };

    let Some(name) = string_arg(args, "faction") else {
        reply(sender, format!("{prefix}{usage}"));
        return;
    };
    let Some((id, faction_name, is_player, members)) = found else {
        let message = util::replace(&missing_tpl, "<argument>", &name);
        reply(sender, format!("{prefix}{message}"));
        return;
    };

    {
        let mut guard = state().write().unwrap();
        guard.claims.remove_all(&id);
        if is_player {
            if let Some(faction) = guard.factions.get_mut(&id) {
                faction.home = None;
            }
        }
    }

    reply(
        sender,
        format!("{prefix}{}", util::replace(&sender_tpl, "<faction>", &faction_name)),
    );

    if !is_player {
        return;
    }

    let message = format!(
        "{prefix}{}",
        util::replace(&faction_tpl, "<player>", &sender.get_name())
    );
    for uuid in members {
        if let Some(id) = pumpkin_plugin_api::uuid::parse(&uuid)
            && let Some(player) = server.get_player_by_uuid(id)
        {
            core::send(&player, &message);
            core::send(&player, format!("{prefix}{home_tpl}"));
        }
    }
}

fn admin_set_kills(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_permission(sender, server, "lazarus:factions.setkills") {
        return;
    }

    let (prefix, usage, missing_tpl, invalid, sender_tpl, faction_tpl, found) = {
        let guard = state().read().unwrap();
        let lang = &guard.raw.language;
        let found = guard
            .factions
            .by_name(&string_arg(args, "faction").unwrap_or_default())
            .filter(|f| f.is_player())
            .map(|f| {
                (
                    f.id.clone(),
                    f.name.clone(),
                    f.members.values().map(|m| m.uuid.clone()).collect::<Vec<_>>(),
                )
            });
        (
            lang.string("FACTION_PREFIX"),
            lang.string("FACTIONS.SET_KILLS_COMMAND.USAGE"),
            lang.string("FACTIONS.FACTION_DOESNT_EXIST"),
            lang.string("COMMANDS.INVALID_NUMBER"),
            lang.string("FACTIONS.SET_KILLS_COMMAND.CHANGED_SENDER"),
            lang.string("FACTIONS.SET_KILLS_COMMAND.CHANGED_FACTION"),
            found,
        )
    };

    let Some(name) = string_arg(args, "faction") else {
        reply(sender, format!("{prefix}{usage}"));
        return;
    };
    let Some(amount) = int_arg(args, "amount") else {
        reply(sender, format!("{prefix}{invalid}"));
        return;
    };
    let Some((id, faction_name, members)) = found else {
        let message = util::replace(&missing_tpl, "<argument>", &name);
        reply(sender, format!("{prefix}{message}"));
        return;
    };

    let amount = amount.abs();
    if let Some(faction) = state().write().unwrap().factions.get_mut(&id) {
        faction.kills = amount as i32;
    }

    let amount = amount.to_string();
    reply(
        sender,
        format!(
            "{prefix}{}",
            util::replace_all(&sender_tpl, &[("<faction>", &faction_name), ("<amount>", &amount)])
        ),
    );

    let message = format!(
        "{prefix}{}",
        util::replace_all(
            &faction_tpl,
            &[("<player>", &sender.get_name()), ("<amount>", &amount)]
        )
    );
    for uuid in members {
        if let Some(id) = pumpkin_plugin_api::uuid::parse(&uuid)
            && let Some(player) = server.get_player_by_uuid(id)
        {
            core::send(&player, &message);
        }
    }
}

fn admin_set_dtr(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_permission(sender, server, "lazarus:factions.dtr") {
        return;
    }
    let Some(name) = string_arg(args, "faction") else {
        return;
    };
    let Some(value) = double_arg(args, "value") else {
        reply(sender, "&cInvalid number.");
        return;
    };

    let mut guard = state().write().unwrap();
    let config = guard.config.clone();
    let Some(id) = guard.factions.by_name(&name).map(|f| f.id.clone()) else {
        reply(sender, util::replace(&guard.lang.faction_doesnt_exist, "<argument>", &name));
        return;
    };
    if let Some(faction) = guard.factions.get_mut(&id) {
        faction.dtr = value.clamp(config.factions.min_dtr, config.factions.max_dtr);
    }
    reply(sender, format!("&eSet &c{name}&e's DTR to &c{value:.2}&e."));
}

fn admin_set_int(sender: &CommandSender, server: &Server, args: &ConsumedArgs, field: AdminField) {
    if !require_permission(sender, server, match field { AdminField::Points => "lazarus:factions.setpoints", AdminField::Balance => "lazarus:factions.setbalance", AdminField::Lives => "lazarus:factions.setlives" }) {
        return;
    }
    let Some(name) = string_arg(args, "faction") else {
        return;
    };
    let Some(value) = int_arg(args, "value") else {
        reply(sender, "&cInvalid number.");
        return;
    };

    let mut guard = state().write().unwrap();
    let Some(id) = guard.factions.by_name(&name).map(|f| f.id.clone()) else {
        reply(sender, util::replace(&guard.lang.faction_doesnt_exist, "<argument>", &name));
        return;
    };
    if let Some(faction) = guard.factions.get_mut(&id) {
        match field {
            AdminField::Points => faction.points = value as i32,
            AdminField::Balance => faction.balance = value,
            AdminField::Lives => faction.lives = value as i32,
        }
    }
    reply(sender, format!("&eUpdated &c{name} &e({value})."));
}

fn admin_force_kick(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_permission(sender, server, "lazarus:factions.forcekick") {
        return;
    }
    let Some(name) = string_arg(args, "player") else {
        return;
    };
    let Some(target) = server.get_player_by_name(&name) else {
        reply(sender, util::replace(&state().read().unwrap().lang.player_not_online, "<player>", &name));
        return;
    };
    let uuid = core::uuid_of(&target);

    let message;
    {
        let mut guard = state().write().unwrap();
        let Some(faction_id) = guard.factions.by_member(&uuid).map(|f| f.id.clone()) else {
            reply(sender, util::replace(&guard.lang.faction_not_in, "<player>", &name));
            return;
        };
        guard.factions.remove_member(&faction_id, &uuid);
        if guard.factions.get(&faction_id).is_some_and(|f| f.members.is_empty()) {
            guard.claims.remove_all(&faction_id);
            guard.factions.disband(&faction_id);
        }
        message = util::replace(&guard.lang.faction_admin_force_kicked, "<player>", &name);
    }
    reply(sender, message);
}

fn admin_force_leader(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !require_permission(sender, server, "lazarus:factions.forceleader") {
        return;
    }
    let Some(name) = string_arg(args, "player") else {
        return;
    };
    let Some(target) = server.get_player_by_name(&name) else {
        reply(sender, util::replace(&state().read().unwrap().lang.player_not_online, "<player>", &name));
        return;
    };
    let uuid = core::uuid_of(&target);

    let mut guard = state().write().unwrap();
    let Some(faction_id) = guard.factions.by_member(&uuid).map(|f| f.id.clone()) else {
        reply(sender, util::replace(&guard.lang.faction_not_in, "<player>", &name));
        return;
    };
    if let Some(faction) = guard.factions.get_mut(&faction_id) {
        for member in faction.members.values_mut() {
            if member.role == Role::Leader {
                member.role = Role::CoLeader;
            }
        }
        if let Some(member) = faction.members.get_mut(&uuid) {
            member.role = Role::Leader;
        }
    }
    reply(sender, format!("&eTransferred leadership to &c{name}&e."));
}

fn admin_freeze(sender: &CommandSender, server: &Server, args: &ConsumedArgs, freeze: bool) {
    if !require_permission(sender, server, if freeze { "lazarus:factions.setfreeze" } else { "lazarus:factions.thaw" }) {
        return;
    }
    let Some(name) = string_arg(args, "faction") else {
        return;
    };

    let mut guard = state().write().unwrap();
    let (Some(id), Some(faction_name), duration) = (
        guard.factions.by_name(&name).map(|f| f.id.clone()),
        guard.factions.by_name(&name).map(|f| f.name.clone()),
        guard.config.factions.freeze_duration_minutes,
    ) else {
        reply(sender, util::replace(&guard.lang.faction_doesnt_exist, "<argument>", &name));
        return;
    };

    if freeze {
        guard.timers.freeze_faction(&id, duration);
        reply(sender, format!("&c{faction_name}&e has been &cfrozen&e."));
    } else {
        guard.timers.unfreeze_faction(&id);
        reply(sender, format!("&c{faction_name}&e has been &athawed&e."));
    }
}

fn admin_save(sender: &CommandSender, server: &Server) {
    if !require_permission(sender, server, "lazarus:factions.save") {
        return;
    }
    crate::state::save_all();
    reply(sender, "&aAll data saved.");
}

fn help(sender: &CommandSender, page: usize) {
    let line = "&7&m                                                                                ";

    let page_lines: [&[&str]; 4] = [
        &[
            " &6&lFaction Help &e(Page 1/4)",
            "",
            " &e/f join &f- &7Accept a join request from an faction.",
            " &e/f chat &f- &7Toggle faction chat only mode on or off.",
            " &e/f claim &f- &7Claim land in the Wilderness.",
            " &e/f create &f- &7Create a faction.",
            " &e/f disband &f- &7Disband your faction.",
            " &e/f home &f- &7Teleport to the faction home.",
            "",
            " &6You are currently on &ePage 1/4.",
            " &6To view other pages, use &e/f help <page>",
        ],
        &[
            " &6&lFaction Help &e(Page 2/4)",
            "",
            " &e/f invite &f- &7Invite a player to the faction.",
            " &e/f kick &f- &7Kick a player from the faction.",
            " &e/f leader &f- &7Sets the new leader for your faction.",
            " &e/f leave &f- &7Leave your current faction.",
            " &e/f list &f- &7See a list of all factions.",
            " &e/f map &f- &7View all claims around your location.",
            "",
            " &6You are currently on &ePage 2/4.",
            " &6To view other pages, use &e/f help <page>",
        ],
        &[
            " &6&lFaction Help &e(Page 3/4)",
            "",
            " &e/f rename &f- &7Change the name of your faction.",
            " &e/f promote &f- &7Promotes a player to a higher role.",
            " &e/f sethome &f- &7Sets the faction home location.",
            " &e/f show &f- &7Get details about a faction.",
            " &e/f unclaim &f- &7Unclaims land from your faction.",
            " &e/f deinvite &f- &7Revoke an invitation to a player.",
            "",
            " &6You are currently on &ePage 3/4.",
            " &6To view other pages, use &e/f help <page>",
        ],
        &[
            " &6&lFaction Help &e(Page 4/4)",
            "",
            " &e/f demote &f- &7Demotes a player to a lower role.",
            " &e/f deposit &f- &7Deposit money to your faction.",
            " &e/f withdraw &f- &7Withdraw money from your faction.",
            " &e/f stuck &f- &7Use it when stuck in enemy teritory.",
            "",
            " &6You are currently on &ePage 4/4.",
            " &6To view other pages, use &e/f help <page>.",
        ],
    ];

    let Some(lines) = page_lines.get(page.saturating_sub(1)) else {
        reply(sender, util::replace("&ePage &c<page> &enot found.", "<page>", &page.to_string()));
        return;
    };

    reply(sender, line);
    for entry in *lines {
        reply(sender, *entry);
    }
    reply(sender, line);
}
