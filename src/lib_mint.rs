use near_contract_standards::non_fungible_token::approval::NonFungibleTokenApproval;
use near_contract_standards::non_fungible_token::core::{
    NonFungibleTokenCore, NonFungibleTokenResolver,
};
use near_contract_standards::non_fungible_token::enumeration::NonFungibleTokenEnumeration;
use near_contract_standards::non_fungible_token::metadata::{
    NFTContractMetadata, NonFungibleTokenMetadataProvider, TokenMetadata, NFT_METADATA_SPEC,
};
use near_contract_standards::non_fungible_token::NonFungibleToken;
use near_sdk::collections::{LazyOption};
use near_sdk::json_types::U128;
use near_sdk::{
    env, near, require, AccountId, BorshStorageKey, PanicOnDefault, NearToken,
};
use borsh::{BorshDeserialize, BorshSerialize};

#[derive(BorshSerialize, BorshDeserialize, BorshStorageKey)]
#[borsh(use_discriminant = false)]
enum StorageKey {
    NonFungibleToken,
    Metadata,
    TokenMetadata,
    Enumeration,
    Approval,
}

const DATA_IMAGE_SVG_NEAR_ICON: &str = "image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 288 288'%3E%3Cpath d='M144 0c-79.5 0-144 64.5-144 144s64.5 144 144 144 144-64.5 144-144S223.5 0 144 0zm72 208c0 4.4-3.6 8-8 8h-48v48c0 4.4-3.6 8-8 8h-48c-4.4 0-8-3.6-8-8v-48h-48c-4.4 0-8-3.6-8-8v-48c0-4.4 3.6-8 8-8h48v-48c0-4.4 3.6-8 8-8h48c4.4 0 8 3.6 8 8v48h48c4.4 0 8 3.6 8 8v48z'/%3E%3C/svg%3E";

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Contract {
    tokens: NonFungibleToken,
    metadata: LazyOption<NFTContractMetadata>,
    pub owner_id: AccountId,
    // authorized_minter может быть прокси-контрактом
    pub authorized_minter: AccountId,
    pub min_mint_price: NearToken, // реальная минимальная цена
    pub next_token_id: u64,
}

#[near]
impl Contract {
    #[init]
    pub fn new(owner_id: AccountId, authorized_minter: AccountId) -> Self {
        require!(!env::state_exists(), "Already initialized");
        let metadata = NFTContractMetadata {
            spec: NFT_METADATA_SPEC.to_string(),
            name: "Easy MINT".to_string(),
            symbol: "MINT".to_string(),
            icon: Some(DATA_IMAGE_SVG_NEAR_ICON.to_string()),
            base_uri: None,
            reference: None,
            reference_hash: None,
        };
        metadata.assert_valid();

        Self {
            tokens: NonFungibleToken::new(
                StorageKey::NonFungibleToken,
                owner_id.clone(),
                Some(StorageKey::TokenMetadata),
                Some(StorageKey::Enumeration),
                Some(StorageKey::Approval),
            ),
            metadata: LazyOption::new(StorageKey::Metadata, Some(&metadata)),
            owner_id,
            authorized_minter, // Устанавливаем прокси-контракт как авторизованного минтера
            min_mint_price: NearToken::from_yoctonear(27_000_000_000_000_000_000_000), // ~0.027 NEAR
            next_token_id: 0,
        }
    }
    // set authorized minter
    pub fn set_authorized_minter(&mut self, new_minter: AccountId) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only the contract owner can change the authorized minter"
        );
        self.authorized_minter = new_minter;
    }
    // set min mint price
    pub fn set_min_mint_price(&mut self, price: NearToken) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "Only the contract owner can change the min mint price"
        );
        self.min_mint_price = price;
    }

    #[payable]
    pub fn nft_mint(
        &mut self,
        token_metadata: TokenMetadata,
    ) -> near_contract_standards::non_fungible_token::Token {
        // 1. Проверяем, что вызывает авторизованный минтер (прокси)
        let caller_id = env::predecessor_account_id();
        require!(
            caller_id == self.authorized_minter,
            "Only authorized minter can call this function"
        );

        // 2. Получаем депозит
        let deposit: NearToken = env::attached_deposit();
        require!(
            deposit >= self.min_mint_price,
            "Deposit must be at least the min mint price (~0.027 NEAR)"
        );

        // 3. Генерируем ID токена
        let token_id = self.next_token_id.to_string();
        let receiver_id = caller_id; // Токен уходит прокси, который должен передать его дальше

        // 4. Минтим NFT
        let token = self.tokens.internal_mint(token_id.clone(), receiver_id.clone(), Some(token_metadata));

        // 5. Увеличиваем счетчик токенов
        self.next_token_id += 1;

        env::log_str(&format!(
            "Minted token {} for {}. Deposit used: {} yoctoNEAR",
            token_id,
            receiver_id,
            deposit.as_yoctonear()
        ));

        // 6. Возвращаем токен
        token
    }
}

// Реализации стандартных NFT трейтов (без изменений)
#[near]
impl NonFungibleTokenCore for Contract {
    #[payable]
    fn nft_transfer(
        &mut self,
        receiver_id: AccountId,
        token_id: near_contract_standards::non_fungible_token::TokenId,
        approval_id: Option<u64>,
        memo: Option<String>,
    ) {
        self.tokens
            .nft_transfer(receiver_id, token_id, approval_id, memo)
    }

    #[payable]
    fn nft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        token_id: near_contract_standards::non_fungible_token::TokenId,
        approval_id: Option<u64>,
        memo: Option<String>,
        msg: String,
    ) -> near_sdk::PromiseOrValue<bool> {
        self.tokens
            .nft_transfer_call(receiver_id, token_id, approval_id, memo, msg)
    }

    fn nft_token(
        &self,
        token_id: near_contract_standards::non_fungible_token::TokenId,
    ) -> Option<near_contract_standards::non_fungible_token::Token> {
        self.tokens.nft_token(token_id)
    }
}

#[near]
impl NonFungibleTokenEnumeration for Contract {
    fn nft_total_supply(&self) -> U128 {
        self.tokens.nft_total_supply().into()
    }

    fn nft_tokens(
        &self,
        from_index: Option<U128>,
        limit: Option<u64>,
    ) -> Vec<near_contract_standards::non_fungible_token::Token> {
        self.tokens.nft_tokens(from_index, limit)
    }

    fn nft_supply_for_owner(&self, account_id: AccountId) -> U128 {
        self.tokens.nft_supply_for_owner(account_id).into()
    }

    fn nft_tokens_for_owner(
        &self,
        account_id: AccountId,
        from_index: Option<U128>,
        limit: Option<u64>,
    ) -> Vec<near_contract_standards::non_fungible_token::Token> {
        self.tokens
            .nft_tokens_for_owner(account_id, from_index, limit)
    }
}

#[near]
impl NonFungibleTokenApproval for Contract {
    fn nft_approve(
        &mut self,
        token_id: near_contract_standards::non_fungible_token::TokenId,
        account_id: AccountId,
        msg: Option<String>,
    ) -> Option<near_sdk::Promise> {
        self.tokens.nft_approve(token_id, account_id, msg)
    }

    fn nft_revoke(
        &mut self,
        token_id: near_contract_standards::non_fungible_token::TokenId,
        account_id: AccountId,
    ) {
        self.tokens.nft_revoke(token_id, account_id)
    }

    fn nft_revoke_all(&mut self, token_id: near_contract_standards::non_fungible_token::TokenId) {
        self.tokens.nft_revoke_all(token_id)
    }

    fn nft_is_approved(
        &self,
        token_id: near_contract_standards::non_fungible_token::TokenId,
        approved_account_id: AccountId,
        approval_id: Option<u64>,
    ) -> bool {
        self.tokens
            .nft_is_approved(token_id, approved_account_id, approval_id)
    }
}

#[near]
impl NonFungibleTokenMetadataProvider for Contract {
    fn nft_metadata(&self) -> NFTContractMetadata {
        self.metadata.get().unwrap()
    }
}

#[near]
impl NonFungibleTokenResolver for Contract {
    fn nft_resolve_transfer(
        &mut self,
        previous_owner_id: AccountId,
        receiver_id: AccountId,
        token_id: near_contract_standards::non_fungible_token::TokenId,
        approved_account_ids: Option<std::collections::HashMap<AccountId, u64>>,
    ) -> bool {
        self.tokens.nft_resolve_transfer(
            previous_owner_id,
            receiver_id,
            token_id,
            approved_account_ids,
        )
    }
}